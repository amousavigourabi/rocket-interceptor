//! This module is responsible for setting up and tearing down the Docker containers who run the validator nodes.

use std::collections::HashMap;
use log::{debug, info};
use std::{env, fs};
use std::io::Read;
use std::io::Write;
use std::time::Duration;

use bollard::container::{CreateContainerOptions, RemoveContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::Docker;
use bollard::network::ConnectNetworkOptions;
use bollard::service::EndpointSettings;
use crate::is_valid_unl_connection;
use crate::packet_client::proto;
use crate::packet_client::PacketClient;
use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;
use serde::Deserialize;
use serde_json::Value;


/// Struct that represents a response of a 'ValidationKeyCreate' request.
#[derive(Debug, Deserialize)]
struct ValidationKeyCreateResponse {
    /// The result of the request.
    result: ValidatorKeyData,
}

/// Struct that represents all the data used for validation by nodes.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct ValidatorKeyData {
    /// The status of the request that was used to make this data.
    pub status: String,
    /// The validation key of a node.
    pub validation_key: String,
    /// The validation private key of a node.
    pub validation_private_key: String,
    /// The validation public key of a node.
    pub validation_public_key: String,
    /// The seed used to make this validation info.
    pub validation_seed: String,
}

/// Struct that represents a Docker container that runs a rippled instance.
#[derive(Debug, Clone)]
pub struct DockerContainer {
    /// The id of the container.
    pub id: Option<String>,
    /// The name of the container.
    pub name: String,
    /// The data of the keys of this node.
    pub key_data: ValidatorKeyData,
}

/// Checks whether a certain `DockerContainer` is available by calling `server_info` and parsing the `success` value.
///
/// # Parameters
/// * 'container' - the container to be checked.
/// * 'docker' - the Docker interface used for API requests.
///
/// # Panics
/// * If the 'id' of the container could not be cloned.
/// * If the creation of the 'server_info' command raised an error.
/// * If the 'server_info' command ran inside the docker container raised an error.
/// * If the output could not be parsed to JSON.
/// * If the JSON could not be parsed to a serde_json object.
async fn check_validator_available(container: DockerContainer, docker: Docker) -> bool {
    let exec = docker
        .clone()
        .create_exec(
            container.id.clone().unwrap().as_ref(),
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
                debug!("{} Available!", container.name.clone());
                return true;
            }
            debug!("{} not available yet...", container.name.clone());
        }
    }
    false
}

/// Struct that represents the whole network of Docker containers.
#[derive(Debug)]
pub struct DockerNetwork {
    /// The network configuration as a proto object.
    pub config: proto::Config,
    /// A Vec of all the individual Docker containers who run a rippled instance.
    pub containers: Vec<DockerContainer>,
    /// A Docker object to access the Docker API.
    docker: Docker,
}

impl DockerNetwork {
    /// Initializes a new DockerNetwork.
    ///
    /// # Parameters
    /// * 'config' - the config to be used to set up the network.
    pub fn new(config: proto::Config) -> DockerNetwork {
        DockerNetwork {
            config,
            containers: Vec::new(),
            docker: Docker::connect_with_local_defaults().unwrap(),
        }
    }

    /// Initializes the docker network by generating keys for each configured node, and starting
    /// them using `bollard`. The containers that were started successfully are appended to
    /// the `containers` field in the struct.
    ///
    /// # Parameters
    /// * 'client' - a PacketClient to send the ValidatorNodeInfo to the controller.
    ///
    /// # Panics
    /// * If an error occurred while sending the ValidatorNodeInfo to the controller.
    pub async fn initialize_network(&mut self, mut client: PacketClient, hostname_prefix: &str) {
        // Stop all running validator nodes before starting new network
        self.stop_network(hostname_prefix).await;
        self.download_image().await;

        self.generate_key_generator_config(hostname_prefix);
        let validator_keys = self.generate_keys(self.config.number_of_nodes as u16, hostname_prefix).await;
        let names_with_keys = self.generate_validator_configs(&validator_keys, hostname_prefix);

        let mut validator_node_info_list = vec![];

        for (name, keys) in names_with_keys.iter() {
            let mut validator_container = DockerContainer {
                id: None,
                name: name.clone(),
                key_data: keys.clone(),
            };
            self.start_validator(&mut validator_container).await;
            info!("Started docker container {}", name.clone());
            validator_node_info_list.push(proto::ValidatorNodeInfo {
                hostname: validator_container.name.clone(),
                status: validator_container.key_data.status.clone(),
                validation_key: validator_container.key_data.validation_key.clone(),
                validation_private_key: validator_container.key_data.validation_private_key.clone(),
                validation_public_key: validator_container.key_data.validation_public_key.clone(),
                validation_seed: validator_container.key_data.validation_seed.clone(),
            });
            self.containers.push(validator_container);
        }
        client
            .send_validator_node_info(validator_node_info_list)
            .await
            .unwrap();
    }

    /// Stops the docker network, by looping over all running containers (`docker ps`)
    /// and stopping all containers that start with `validator_` or `key_generator`.
    ///
    /// # Panics
    /// * If it could not fetch the list of running containers from the Docker API.
    /// * If it could not terminate any running container.
    pub async fn stop_network(&self, hostname_prefix: &str) {
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
                    let validator_name_start = format!("/{}_validator_", hostname_prefix);
                    let key_generator_name = format!("/{}_key_generator", hostname_prefix);
                    if name.starts_with(&validator_name_start) || name.eq(&key_generator_name) {
                        debug!(
                            "Stopping container (auto removed): {}",
                            container.id.clone().unwrap().as_str()
                        );
                        self.docker
                            .stop_container(container.id.clone().unwrap().as_str(), None)
                            .await
                            .unwrap();

                        // Poll until the container is no longer in the list of running containers
                        loop {
                            let containers =
                                self.docker.list_containers::<String>(None).await.unwrap();
                            if containers.iter().all(|c| c.id != container.id) {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
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
        for container in self.containers.clone() {
            let _docker = arc.clone();
            let t = tokio::spawn(async move {
                loop {
                    if check_validator_available(container.clone(), _docker.clone()).await {
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

    /// Downloads the 'isvanloon/rippled-no-sig-check:latest' image from DockerHub.
    ///
    /// # Panics
    /// * If an error occurred while downloading the image.
    async fn download_image(&mut self) {
        self.docker
            .create_image(
                Some(CreateImageOptions {
                    from_image: option_env!("ROCKET_XRPLD_DOCKER_CONTAINER").unwrap_or("xrpllabsofficial/xrpld:2.4.0"),
                    ..Default::default()
                }),
                None,
                None,
            )
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
    }

    /// Starts a validator node.
    /// It binds specific ports of the container to be able to communicate with the nodes.
    /// Besides, it starts the validator node with a specified ledger to have all amendments already included.
    ///
    /// # Parameters
    /// * 'container' - the container to be started.
    ///
    /// # Panics
    /// * If it could not format the directory path to the 'config' directory.
    /// * If the Docker container who runs the validator could not be created or started.
    async fn start_validator(&self, container: &mut DockerContainer) {


        // Create exposed ports without binding them to host
        let mut exposed_ports = HashMap::new();
        exposed_ports.insert("51235/tcp", HashMap::new()); // Peer port
        exposed_ports.insert("6006/tcp", HashMap::new()); // ws admin port
        exposed_ports.insert("5005/tcp", HashMap::new()); // rpc port

        let create_options = CreateContainerOptions {
            name: container.name.as_str(),
            ..Default::default()
        };

        let network_path = env::var("ROCKET_NETWORK_MOUNT").unwrap();
        let network_name = "rocket_net";

        let container_config = bollard::container::Config {
            hostname: Some(container.name.as_str()),
            image: Some(option_env!("ROCKET_XRPLD_DOCKER_CONTAINER").unwrap_or("xrpllabsofficial/xrpld:2.4.0")),
            env: Some(vec!["ENV_ARGS=--start --ledgerfile /config/ledger.json"]),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                auto_remove: Some(true),
                mounts: Some(vec![Mount {
                    target: Some(String::from("/config")),
                    source: Some(format!("{}/network/{}/config", network_path, container.name.as_str())),
                    typ: Some(MountTypeEnum::BIND),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        match self
            .docker
            .create_container::<&str, &str>(Some(create_options), container_config)
            .await
        {
            Ok(container_response) => {
                let id = container_response.id;
                
                match self.docker.connect_network(
                    network_name,
                    ConnectNetworkOptions {
                        container: id.clone(),
                        endpoint_config: EndpointSettings::default(),
                    },
                ).await
                {
                    Ok(_) => {
                        match self.docker.start_container::<String>(&id, None).await {
                            Ok(_) => {
                                container.id = Some(id.clone());
                            }
                            Err(e) => {
                                panic!("Failed to start the xrpld container, try checking your hostname_prefix configuration values to make sure they are not bound by another container: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        panic!("Failed to connect the xrpld container to the network: {}", e);
                    }
                }
            }
            Err(e) => {
                panic!("Failed to create container: {}", e);
            }
        }
    }

    /// Generates `n` validator keys using a `rippled` instance.
    ///
    /// # Parameters
    /// * 'n' - the amount of validator keys to generate.
    ///
    /// # Panics
    /// * If the JSON response from `rippled` cannot be correctly deserialized to the `ValidationKeyCreateResponse` struct.
    /// * If an error occurred while creating or starting the Docker container who generates the keys.
    /// * If an error occurred while creating or executing the 'validation_create' command.
    /// * If an error occurred while removing the Docker container who generated the keys.
    async fn generate_keys(&self, n: u16, hostname_prefix: &str) -> Vec<ValidatorKeyData> {
        let container_name = format!("{}_key_generator", hostname_prefix); 
        let create_options = CreateContainerOptions {
            name: container_name.as_str(),
            ..Default::default()
        };

        let network_path = env::var("ROCKET_NETWORK_MOUNT").unwrap();

        let container_config = bollard::container::Config {
            hostname: Some(container_name.as_str()),
            image: Some(option_env!("ROCKET_XRPLD_DOCKER_CONTAINER").unwrap_or("xrpllabsofficial/xrpld:2.4.0")),
            host_config: Some(HostConfig {
                auto_remove: Some(true),
                mounts: Some(vec![Mount {
                    target: Some(String::from("/config")),
                    source: Some(format!("{}/network/{}/config", network_path, container_name.as_str() )),
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

        loop {
            if check_validator_available(
                DockerContainer {
                    id: Some(id.clone()),
                    name: container_name.clone(),
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
    /// are saved to /network/validators/\<name\>.
    ///
    /// # Panics
    /// * If the `rippled_base.cfg` cannot be read.
    /// * If the config could not be written to disk (no permissions/directory does not exist).
    fn generate_validator_configs(
        &self,
        keys: &[ValidatorKeyData],
        hostname_prefix: &str,
    ) -> Vec<(String, ValidatorKeyData)> {
        // let network_path = env::var("ROCKET_NETWORK_MOUNT").unwrap_or(current_dir.to_str().unwrap().to_string());
        let network_path = env::var("ROCKET_NETWORK_MOUNT").unwrap();

        let base_config_path = format!("{}/network/rippled_base.cfg", network_path);
        let ledger_json_path = format!("{}/network/ledger.json", network_path);
        let base_config_file = fs::File::open(&base_config_path);

        let mut base_config_contents = String::new();
        base_config_file
            .unwrap()
            .read_to_string(&mut base_config_contents)
            .unwrap_or_else(|_| panic!("Could not read file {}", base_config_path));

        let mut ret: Vec<(String, ValidatorKeyData)> = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let container_name = format!("{}_validator_{}",hostname_prefix, i);
            let new_config_contents = base_config_contents
                .clone()
                .replace("{validation_seed}", key.validation_seed.as_str());

            let config_dir = format!("{}/network/{}/config",network_path , container_name.clone());
            fs::create_dir_all(config_dir.as_str()).expect("Could not create directory.");

            let mut config_file =
                fs::File::create(format!("{}/rippled.cfg", config_dir.as_str())).unwrap();
            config_file
                .write_all(new_config_contents.as_bytes())
                .expect("Could not write to config file");

            let mut validators_file =
                fs::File::create(format!("{}/validators.txt", config_dir)).unwrap();

            let unl_public_keys: Vec<String> = keys
                .iter()
                .enumerate()
                .filter(|(j, _)| {
                    is_valid_unl_connection(i as u32, *j as u32, &self.config.unl_partitions)
                })
                .map(|(_, k)| k.validation_public_key.to_string())
                .collect();

            validators_file
                .write_all(format!("[validators]\n{}", unl_public_keys.join("\n")).as_bytes())
                .expect("Could not write to config file");

            fs::copy(&ledger_json_path, format!("{}/ledger.json", config_dir)).unwrap();

            ret.push((container_name, key.clone()));
        }
        ret
    }

    fn generate_key_generator_config(
        &self,
        hostname_prefix: &str,
    ){
        let network_path = env::var("ROCKET_NETWORK_MOUNT").unwrap();
        debug!("network_path: {}", network_path);
        let base_config_path = format!("{}/network/key_generator/config/rippled.cfg", network_path);

        let config_dir = format!("{}/network/{}_key_generator/config", network_path, hostname_prefix);
        fs::create_dir_all(config_dir.as_str()).expect("Could not create directory.");

        fs::copy(&base_config_path, format!("{}/rippled.cfg", config_dir)).unwrap();
    }
}

#[cfg(test)]
mod integration_tests_docker {
    use super::*;
    use crate::packet_client;
    use crate::packet_client::proto::Config;

    fn docker_network_setup() -> DockerNetwork {
        let config = Config {
            hostname_prefix: "TEST".to_string(),
            number_of_nodes: 3,
            net_partitions: vec![],
            unl_partitions: vec![],
        };
        DockerNetwork::new(config)
    }

    // Note: This test requires running a docker engine in clean state
    // Tests the generate_keys function; assert that the keys are generated and the container is removed
    #[tokio::test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    async fn test_generate_keys() {
        let docker_network = docker_network_setup();
        assert_eq!(
            docker_network
                .docker
                .list_containers::<String>(None)
                .await
                .unwrap()
                .len(),
            0,
            "This test requires a clean docker starting state"
        );
        let keys = docker_network.generate_keys(3, "TEST").await;
        assert_eq!(keys.len(), 3);
        assert_eq!(
            docker_network
                .docker
                .list_containers::<String>(None)
                .await
                .unwrap()
                .len(),
            0,
            "Docker containers were not removed correctly"
        );
    }

    // Tests the generate_validator_configs function; assert that the config files are correctly created
    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn test_generate_validator_configs() {
        let keys = vec![
            ValidatorKeyData {
                status: "success".to_string(),
                validation_key: "val_key1".to_string(),
                validation_private_key: "priv_key1".to_string(),
                validation_public_key: "pub_key1".to_string(),
                validation_seed: "seed1".to_string(),
            },
            ValidatorKeyData {
                status: "success".to_string(),
                validation_key: "val_key2".to_string(),
                validation_private_key: "priv_key2".to_string(),
                validation_public_key: "pub_key2".to_string(),
                validation_seed: "seed2".to_string(),
            },
        ];
        let docker_network = docker_network_setup();
        let configs = docker_network.generate_validator_configs(&keys, "TEST");
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].0, "TEST_validator_0");
        assert_eq!(configs[1].0, "TEST_validator_1");
        assert_eq!(configs[0].1, keys[0]);
        assert_eq!(configs[1].1, keys[1]);

        // check if the files for the first validator are created
        let dir1 = "./network/validators/TEST_validator_0/config/";
        assert!(
            fs::metadata(format!("{}{}", &dir1, "ledger.json")).is_ok(),
            "File not found, path: {}{}",
            &dir1,
            "ledger.json"
        );
        assert!(
            fs::metadata(format!("{}{}", &dir1, "rippled.cfg")).is_ok(),
            "File not found, path: {}{}",
            &dir1,
            "rippled.cfg"
        );
        let validators_txt_file_path1 = format!("{}{}", &dir1, "validators.txt");
        assert!(
            fs::metadata(&validators_txt_file_path1).is_ok(),
            "File not found, path: {}",
            &validators_txt_file_path1
        );
        let mut file = fs::File::open(&validators_txt_file_path1).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(
            contents, "[validators]\npub_key2",
            "Contents were not generated correctly"
        );

        // check if the files for the second validator are created
        let dir2 = "./network/validators/TEST_validator_1/config/";
        assert!(
            fs::metadata(format!("{}{}", &dir2, "ledger.json")).is_ok(),
            "File not found, path: {}{}",
            &dir2,
            "ledger.json"
        );
        assert!(
            fs::metadata(format!("{}{}", &dir2, "rippled.cfg")).is_ok(),
            "File not found, path: {}{}",
            &dir2,
            "rippled.cfg"
        );
        let validators_txt_file_path2 = format!("{}{}", &dir2, "validators.txt");
        assert!(
            fs::metadata(&validators_txt_file_path2).is_ok(),
            "File not found, path: {}",
            &validators_txt_file_path2
        );
        let mut file = fs::File::open(&validators_txt_file_path2).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(
            contents, "[validators]\npub_key1",
            "Contents were not generated correctly"
        );
    }

    // Note: This test requires running a docker engine in clean state and the controller to be running
    // Tests the initialize_network function; assert that the network is correctly initialized with the correct amount of nodes and names.
    // Also tests the stop_network function
    #[tokio::test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    async fn test_initialize_network() {
        let mut docker_network = docker_network_setup();
        assert_eq!(
            docker_network
                .docker
                .list_containers::<String>(None)
                .await
                .unwrap()
                .len(),
            0,
            "This test requires a clean docker starting state"
        );
        let client = match packet_client::PacketClient::new().await {
            Ok(client) => client,
            error => panic!("Error creating client: {:?}", error),
        };
        docker_network.initialize_network(client, "TEST").await;
        assert_eq!(
            docker_network.containers.len(),
            3,
            "Not all containers were started"
        );

        let running_containers = docker_network
            .docker
            .list_containers::<String>(None)
            .await
            .unwrap();
        assert_eq!(
            running_containers.len(),
            3,
            "Not all containers were started"
        );

        let re = regex::Regex::new(r"^/validator_\d+$").unwrap();
        for container in running_containers {
            if let Some(names) = container.names {
                for name in names {
                    assert!(
                        re.is_match(&name),
                        "Container name does not match expected pattern: {}",
                        name
                    );
                }
            }
        }

        docker_network.stop_network("TEST").await;
        assert_eq!(
            docker_network
                .docker
                .list_containers::<String>(None)
                .await
                .unwrap()
                .len(),
            0,
            "Docker containers were not stopped correctly"
        );
    }
}
