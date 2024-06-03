use log::{debug, warn};
use std::env::current_dir;
use std::fs;
use std::io::Read;
use std::io::{ErrorKind, Write};
use std::sync::Arc;
use std::time::Duration;

use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, Mount, MountTypeEnum, PortBinding, PortMap};
use bollard::Docker;

use crate::packet_client::PacketClient;
use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

const IMAGE: &str = "isvanloon/rippled-no-sig-check:latest";

#[derive(Debug, Deserialize)]
pub struct NetworkConfig {
    pub base_port: Option<u16>,
    base_port_ws: Option<u16>,
    base_port_ws_admin: Option<u16>,
    base_port_rpc: Option<u16>,
    number_of_nodes: u16,
}

/// Reads the `network-config.toml` file and returns a parsed `NetworkConfig` struct.
///
/// # Panics
/// This function panics if there is an error with reading the config file, or
/// if the configuration does not adhere to the correct format.
pub fn get_config() -> NetworkConfig {
    let config_file = "network-config.toml";
    let contents = match fs::read_to_string(config_file) {
        Ok(c) => c,
        Err(e) => match e.kind() {
            ErrorKind::NotFound => {
                warn!("'network-config.toml' file not found, defaulting to a network of 2 nodes.");
                String::from("number_of_nodes = 2")
            }
            _ => {
                panic!("Could not read config file `{}`", config_file);
            }
        },
    };

    let network_config: NetworkConfig = match toml::from_str(&contents) {
        Ok(d) => d,
        Err(_) => {
            panic!("Unable to parse config file `{}`", config_file);
        }
    };
    network_config
}

#[derive(Debug, Deserialize)]
struct ValidationKeyCreateResponse {
    result: ValidatorKeyData,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ValidatorKeyData {
    pub status: String,
    pub validation_key: String,
    pub validation_private_key: String,
    pub validation_public_key: String,
    pub validation_seed: String,
}

#[derive(Debug, Clone)]
pub struct DockerContainer {
    pub id: Option<String>,
    pub name: String,
    pub port_peer: u16,
    pub port_ws: u16,
    pub port_ws_admin: u16,
    pub port_rpc: u16,
    pub key_data: ValidatorKeyData,
}

/// Checks whether a certain `DockerContainer` is available by calling `server_info`
/// and parsing the `success` value.
async fn check_validator_available(c: DockerContainer, docker: Docker) -> bool {
    let exec = docker
        .clone()
        .create_exec(
            c.id.clone().unwrap().as_ref(),
            CreateExecOptions {
                attach_stdout: Some(true),
                cmd: Some(vec!["rippled", "server_info"]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
    if let StartExecResults::Attached { mut output, .. } =
        docker.clone().start_exec(&exec, None).await.unwrap()
    {
        while let Some(Ok(msg)) = output.next().await {
            let json_str = String::from_utf8(msg.as_ref().to_vec()).unwrap();
            let server_info_data: Value = serde_json::from_str(json_str.as_str()).unwrap();
            if server_info_data["result"]["status"] == "success" {
                debug!("{} Available!", c.name.clone());
                return true;
            }
            debug!("{} not available yet...", c.name.clone());
        }
    }
    false
}

#[derive(Debug)]
pub struct DockerNetwork {
    pub config: NetworkConfig,
    pub containers: Vec<DockerContainer>,
    docker: Docker,
}

impl DockerNetwork {
    pub fn new(config: NetworkConfig) -> DockerNetwork {
        DockerNetwork {
            config,
            containers: Vec::new(),
            docker: Docker::connect_with_local_defaults().unwrap(),
        }
    }

    /// Initializes the docker network by generating keys for each configured node, and starting
    /// them using `bollard`. The containers that were started successfully are appended to
    /// the `containers` field in the struct.
    pub async fn initialize_network(&mut self, client: Arc<Mutex<PacketClient>>) {
        // Stop all running validator nodes before starting new network
        self.stop_network().await;
        self.download_image().await;

        let validator_keys = self.generate_keys(self.config.number_of_nodes).await;
        let names_with_keys = self.generate_validator_configs(&validator_keys);

        let base_port_peer = self.config.base_port.unwrap_or(60000);
        let base_port_ws = self.config.base_port_ws.unwrap_or(61000);
        let base_port_ws_admin = self.config.base_port_ws_admin.unwrap_or(62000);
        let base_port_rpc = self.config.base_port_rpc.unwrap_or(63000);

        let mut validator_node_info_list = vec![];

        for (i, (name, keys)) in names_with_keys.iter().enumerate() {
            let mut validator_container = DockerContainer {
                id: None,
                name: name.clone(),
                port_peer: base_port_peer + i as u16,
                port_ws: base_port_ws + i as u16,
                port_ws_admin: base_port_ws_admin + i as u16,
                port_rpc: base_port_rpc + i as u16,
                key_data: keys.clone(),
            };
            self.start_validator(&mut validator_container).await;
            debug!("Started docker container {}", name.clone());
            validator_node_info_list.push(crate::packet_client::proto::ValidatorNodeInfo {
                peer_port: validator_container.port_peer as u32,
                ws_public_port: validator_container.port_ws as u32,
                ws_admin_port: validator_container.port_ws_admin as u32,
                rpc_port: validator_container.port_rpc as u32,
                status: validator_container.key_data.status.clone(),
                validation_key: validator_container.key_data.validation_public_key.clone(),
                validation_private_key: validator_container.key_data.validation_private_key.clone(),
                validation_public_key: validator_container.key_data.validation_public_key.clone(),
                validation_seed: validator_container.key_data.validation_seed.clone(),
            });
            self.containers.push(validator_container);
        }
        client
            .lock()
            .await
            .send_validator_node_info(validator_node_info_list)
            .await
            .unwrap();
    }

    /// Stops the docker network, by looping over all running containers (`docker ps`)
    /// and stopping all containers that start with `validator_` or `key_generator`
    pub async fn stop_network(&self) {
        let running_containers = self
            .docker
            .list_containers::<String>(None)
            .await
            .expect("Could not fetch container list from docker");
        for container in running_containers {
            if let Some(names) = container.names {
                for name in names {
                    debug!("{}", name);
                    // Docker container names always start with a slash
                    if name.starts_with("/validator_") || name.eq("/key_generator") {
                        debug!(
                            "Stopping container (auto removed): {}",
                            container.id.clone().unwrap().as_str()
                        );
                        self.docker
                            .stop_container(container.id.clone().unwrap().as_str(), None)
                            .await
                            .unwrap()
                    }
                }
            }
        }
    }

    /// Loop over all containers in `self`, and poll them every 500ms, until
    /// all containers are available.
    pub async fn wait_for_startup(&self) {
        let mut threads = vec![];
        let arc = self.docker.clone();
        for c in self.containers.clone() {
            let _docker = arc.clone();
            let t = tokio::spawn(async move {
                loop {
                    if check_validator_available(c.clone(), _docker.clone()).await {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            });
            threads.push(t);
        }

        for t in threads {
            t.await.expect("Wait for startup failed for thread");
        }
    }

    async fn download_image(&mut self) {
        self.docker
            .create_image(
                Some(CreateImageOptions {
                    from_image: IMAGE,
                    ..Default::default()
                }),
                None,
                None,
            )
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
    }

    async fn start_validator(&self, container: &mut DockerContainer) {
        let mut port_map = PortMap::new();
        port_map.insert(
            String::from("51235/tcp"),
            Some(vec![PortBinding {
                host_port: Some(container.port_peer.to_string()),
                ..Default::default()
            }]),
        );
        port_map.insert(
            String::from("6005/tcp"),
            Some(vec![PortBinding {
                host_port: Some(container.port_ws.to_string()),
                ..Default::default()
            }]),
        );
        port_map.insert(
            String::from("6006/tcp"),
            Some(vec![PortBinding {
                host_port: Some(container.port_ws_admin.to_string()),
                ..Default::default()
            }]),
        );
        port_map.insert(
            String::from("5005/tcp"),
            Some(vec![PortBinding {
                host_port: Some(container.port_rpc.to_string()),
                ..Default::default()
            }]),
        );

        let create_options = CreateContainerOptions {
            name: container.name.as_str(),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(IMAGE),
            env: Some(vec![
                "ENV_ARGS=--start --ledgerfile /etc/opt/ripple/ledger.json",
            ]),
            host_config: Some(HostConfig {
                auto_remove: Some(true),
                port_bindings: Some(port_map),
                mounts: Some(vec![Mount {
                    target: Some(String::from("/config")),
                    source: Some(format!(
                        "{}/network/validators/{}/config",
                        current_dir().unwrap().to_str().unwrap(),
                        container.name.as_str()
                    )),
                    typ: Some(MountTypeEnum::BIND),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let id = self
            .docker
            .create_container::<&str, &str>(Some(create_options), container_config)
            .await
            .unwrap()
            .id;
        self.docker
            .start_container::<String>(&id, None)
            .await
            .unwrap();

        container.id = Some(id.clone());
    }

    /// Generates `n` validator keys using a `rippled` instance.
    ///
    /// # Panics
    /// This function panics if the JSON response from `rippled` cannot be correctly
    /// deserialized to the `ValidationKeyCreateResponse` struct.
    async fn generate_keys(&self, n: u16) -> Vec<ValidatorKeyData> {
        let container_name = String::from("key_generator");
        let create_options = CreateContainerOptions {
            name: container_name.as_str(),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(IMAGE),
            host_config: Some(HostConfig {
                auto_remove: Some(true),
                mounts: Some(vec![Mount {
                    target: Some(String::from("/config")),
                    source: Some(format!(
                        "{}/network/{}/config",
                        current_dir().unwrap().to_str().unwrap(),
                        container_name.as_str()
                    )),
                    typ: Some(MountTypeEnum::BIND),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let id = self
            .docker
            .create_container::<&str, &str>(Some(create_options), container_config)
            .await
            .unwrap()
            .id;

        self.docker
            .start_container::<String>(&id, None)
            .await
            .unwrap();

        // Quick implementation of checking whether the key_generator container
        // is available.
        loop {
            if check_validator_available(
                DockerContainer {
                    id: Some(id.clone()),
                    name: container_name.clone(),
                    port_peer: 0,
                    port_ws: 0,
                    port_ws_admin: 0,
                    port_rpc: 0,
                    key_data: ValidatorKeyData {
                        status: "success".to_string(),
                        validation_key: "".to_string(),
                        validation_private_key: "".to_string(),
                        validation_public_key: "".to_string(),
                        validation_seed: "".to_string(),
                    },
                },
                self.docker.clone(),
            )
            .await
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Generate the keys and parse the output
        let mut key_vec: Vec<ValidatorKeyData> = Vec::new();
        for _ in 0..n {
            let exec = self
                .docker
                .create_exec(
                    &id,
                    CreateExecOptions {
                        attach_stdout: Some(true),
                        cmd: Some(vec!["rippled", "validation_create"]),
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .id;
            if let StartExecResults::Attached { mut output, .. } =
                self.docker.start_exec(&exec, None).await.unwrap()
            {
                while let Some(Ok(msg)) = output.next().await {
                    let json_str = String::from_utf8(msg.as_ref().to_vec()).unwrap();
                    let validator_key_data: ValidationKeyCreateResponse =
                        serde_json::from_str(json_str.as_str()).unwrap();

                    key_vec.push(validator_key_data.result);
                }
            }
        }

        self.docker
            .remove_container(
                &id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        key_vec
    }

    /// Generates and writes the config files for every key in `keys` to disk. The configurations
    /// are saved to /network/validators/<name>
    ///
    /// # Panics
    /// This function panics when the `rippled_base.cfg` cannot be read, or the config cannot
    /// be written to disk (no permissions/directory does not exist)
    fn generate_validator_configs(
        &self,
        keys: &[ValidatorKeyData],
    ) -> Vec<(String, ValidatorKeyData)> {
        let base_config_path = "network/rippled_base.cfg";
        let ledger_json_path = "network/ledger.json";
        let base_config_file = fs::File::open(base_config_path);

        let mut base_config_contents = String::new();
        base_config_file
            .unwrap()
            .read_to_string(&mut base_config_contents)
            .unwrap_or_else(|_| panic!("Could not read file {}", base_config_path));

        let mut ret: Vec<(String, ValidatorKeyData)> = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let container_name = format!("validator_{}", i);
            let new_config_contents = base_config_contents
                .clone()
                .replace("{validation_seed}", key.validation_seed.as_str());

            let config_dir = format!("network/validators/{}/config", container_name.clone());
            fs::create_dir_all(config_dir.as_str()).expect("Could not create directory.");

            let mut config_file =
                fs::File::create(format!("{}/rippled.cfg", config_dir.as_str())).unwrap();
            config_file
                .write_all(new_config_contents.as_bytes())
                .expect("Could not write to config file");

            let mut validators_file =
                fs::File::create(format!("{}/validators.txt", config_dir)).unwrap();
            let mut keys_except_current = keys.to_vec();
            keys_except_current.remove(i);

            let public_keys: Vec<String> = keys_except_current
                .iter()
                .map(|k| k.validation_public_key.to_string())
                .collect();
            validators_file
                .write_all(format!("[validators]\n{}", public_keys.join("\n")).as_bytes())
                .expect("Could not write to config file");

            fs::copy(ledger_json_path, format!("{}/ledger.json", config_dir)).unwrap();

            ret.push((container_name, key.clone()));
        }
        ret
    }
}
