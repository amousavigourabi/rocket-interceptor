use log::{debug, error, info, warn};
use serde::Deserialize;
use std::env::current_dir;
use std::fmt::format;
use std::fs;
use std::io::{Error, Read};
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};
use std::ptr::null;
use std::thread::sleep;
use std::time::Duration;

#[derive(Deserialize)]
struct NetworkConfig {
    number_of_nodes: u16,
}

#[derive(Debug, Deserialize)]
struct ValidationKeyCreateResponse {
    result: ValidatorKeyData,
}

#[derive(Debug, Deserialize, Clone)]
struct ValidatorKeyData {
    status: String,
    validation_key: String,
    validation_private_key: String,
    validation_public_key: String,
    validation_seed: String,
}

fn start_validator(name: &str, peer_port: u16) {
    Command::new("docker")
        .arg("run")
        .arg("-d")
        .arg("--rm")
        .args(["-p", format!("{}:51235", peer_port).as_str()])
        .args(["--name", name])
        .args([
            "--mount",
            format!(
                "type=bind,source={}/network/validators/{}/config,target=/config",
                current_dir().unwrap().to_str().unwrap(),
                name
            )
            .as_str(),
        ])
        .arg("isvanloon/rippled-no-sig-check")
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Failed to start docker container");
}

fn stop_container_by_name(name: &str) {
    Command::new("docker")
        .arg("stop")
        .arg(name)
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Could not stop container: key_generator");
}

fn stop_all_containers() {
    Command::new("docker")
        .arg("container")
        .arg("stop")
        .arg(r#"$(docker container list -q)"#)
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Could not stop container: key_generator");
}

fn generate_keys(n: u16) -> Vec<ValidatorKeyData> {
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

    stop_container_by_name("key_generator");
    key_vec
}

pub fn generate_validator_configs(keys: &Vec<ValidatorKeyData>) -> Vec<(String, ValidatorKeyData)> {
    let base_config_path = "network/rippled_base.cfg";
    let base_config_file = fs::File::open(base_config_path);

    let mut base_config_contents = String::new();
    base_config_file
        .unwrap()
        .read_to_string(&mut base_config_contents)
        .expect(format!("Could not read file {}", base_config_path).as_str());

    let mut ret: Vec<(String, ValidatorKeyData)> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        let container_name = format!("validator_{}", i);
        let new_config_contents = base_config_contents
            .clone()
            .replace("{validation_seed}", key.validation_seed.as_str());

        let mut config_dir = format!("network/validators/{}/config", container_name.clone());
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

pub fn initialize_network() {
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

    debug!(
        "Starting a network of: {} validator nodes.",
        network_config.number_of_nodes
    );

    let validator_keys = generate_keys(network_config.number_of_nodes);
    let names_with_keys = generate_validator_configs(&validator_keys);

    for (i, (name, keys)) in names_with_keys.iter().enumerate() {
        start_validator(name, (6000 + i) as u16)
    }
}
