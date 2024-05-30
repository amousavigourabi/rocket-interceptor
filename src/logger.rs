use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        use $crate::logger;
        logger::log(&format!($($arg)*));
    }}
}

// This logger is for now only used to log the time, sent packet and received response, formatted in a csv file.
lazy_static::lazy_static! {
    static ref FILE: Mutex<std::fs::File> = {
        let now = Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let logs_dir = format!("./logs/{}", timestamp);
        fs::create_dir_all(&logs_dir).expect("Failed to create logs directory");

        let file_path = format!("{}/execution.csv", logs_dir);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .expect("Failed to open log file");

        writeln!(file, "time,sent_data,from_port,to_port,received_data,action").unwrap();

        Mutex::new(file)
    };
}

pub fn log(message: &str) {
    let mut file = FILE.lock().unwrap();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    writeln!(file, "{},{}", now, message).unwrap();
}
