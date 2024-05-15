use log::{debug, warn};
use serde::Deserialize;
use std::env::current_dir;
use std::fs;
use std::io::Read;
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct NetworkConfig {
    base_port: Option<u16>,
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

#[derive(Debug)]
pub struct DockerContainer {
    pub name: String,
    pub port_peer: u16,
    pub port_ws: u16,
    pub port_ws_admin: u16,
    pub port_rpc: u16,
    pub key_data: ValidatorKeyData,
}

#[derive(Debug)]
pub struct DockerNetwork {
    pub config: NetworkConfig,
    pub containers: Vec<DockerContainer>,
}

impl DockerNetwork {
    pub fn new(config: NetworkConfig) -> DockerNetwork {
        DockerNetwork {
            containers: Vec::new(),
            config,
        }
    }

    /// Initializes the docker network by generating keys for each configured node, and starting
    /// them using `docker run`. The containers that were started successfully are appended to
    /// the `containers` field in the struct.
    pub fn initialize_network(&mut self) {
        let validator_keys = self.generate_keys(self.config.number_of_nodes);
        let names_with_keys = self.generate_validator_configs(&validator_keys);

        let base_port_peer = self.config.base_port.unwrap_or(60000);
        let base_port_ws = self.config.base_port_ws.unwrap_or(61000);
        let base_port_ws_admin = self.config.base_port_ws_admin.unwrap_or(62000);
        let base_port_rpc = self.config.base_port_rpc.unwrap_or(63000);

        for (i, (name, keys)) in names_with_keys.iter().enumerate() {
            let validator_container = DockerContainer {
                name: name.clone(),
                port_peer: base_port_peer + i as u16,
                port_ws: base_port_ws + i as u16,
                port_ws_admin: base_port_ws_admin + i as u16,
                port_rpc: base_port_rpc + i as u16,
                key_data: keys.clone(),
            };
            self.start_validator(&validator_container);
            self.containers.push(validator_container);
        }
    }

    pub fn stop_network(&self) {
        Command::new("docker")
            .arg("stop")
            .arg(r#"$(docker container list -q)"#)
            .stdout(Stdio::null())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Could not stop container: key_generator");
    }

    fn start_validator(&self, container: &DockerContainer) {
        let stat = Command::new("docker")
            .arg("run")
            .arg("-d")
            .arg("--rm")
            .args(["-p", format!("{}:51235", container.port_peer).as_str()])
            .args(["-p", format!("{}:6005", container.port_ws).as_str()])
            .args(["-p", format!("{}:6006", container.port_ws_admin).as_str()])
            .args(["-p", format!("{}:5005", container.port_rpc).as_str()])
            .args(["--name", container.name.as_str()])
            .args([
                "--mount",
                format!(
                    "type=bind,source={}/network/validators/{}/config,target=/config",
                    current_dir().unwrap().to_str().unwrap(),
                    container.name.as_str()
                )
                .as_str(),
            ])
            .arg("isvanloon/rippled-no-sig-check")
            .stdout(Stdio::null())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to start docker container");

        match stat.code() {
            Some(0) => {
                debug!("Started container {} successfully", container.name.as_str())
            }
            Some(i) => {
                panic!("Starting docker container failed, exit code: {}", i)
            }
            None => {
                panic!(
                    "No exit code when starting container {}",
                    container.name.as_str()
                )
            }
        }
    }

    fn stop_container_by_name(&self, name: &str) {
        Command::new("docker")
            .arg("stop")
            .arg(name)
            .stdout(Stdio::null())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Could not stop container: key_generator");
    }

    /// Generates `n` validator keys using a `rippled` instance.
    ///
    /// # Panics
    /// This function panics if the JSON response from `rippled` cannot be correctly
    /// deserialized to the `ValidationKeyCreateResponse` struct.
    fn generate_keys(&self, n: u16) -> Vec<ValidatorKeyData> {
        Command::new("docker")
            .arg("run")
            .arg("-d")
            .arg("--rm")
            .args(["--name", "key_generator"])
            .args([
                "--mount",
                format!(
                    "type=bind,source={}/network/key_generator/config,target=/config",
                    current_dir().unwrap().to_str().unwrap()
                )
                .as_str(),
            ])
            .arg("isvanloon/rippled-no-sig-check")
            .stdout(Stdio::null())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("Failed to start docker container");

        //TODO: Find a better way to wait for the container to finish loading completely
        sleep(Duration::from_secs(2));

        let mut key_vec: Vec<ValidatorKeyData> = Vec::new();
        for _ in 0..n {
            let cmd_result = Command::new("docker")
                .arg("exec")
                .arg("key_generator")
                .arg("/bin/bash")
                .arg("-c")
                .arg(r#"rippled validation_create"#)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .expect("Could not execute command in docker container");

            let json_str = String::from_utf8(cmd_result.stdout).unwrap();
            let validator_key_data: ValidationKeyCreateResponse =
                serde_json::from_str(json_str.as_str()).unwrap();

            key_vec.push(validator_key_data.result);
        }

        self.stop_container_by_name("key_generator");
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

            ret.push((container_name, key.clone()));
        }
        ret
    }
}
