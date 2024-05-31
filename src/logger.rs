use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

#[macro_export]
macro_rules! log {
    ($file:expr, $($arg:tt)*) => {{
        use $crate::logger;
        logger::log(&($file), &format!($($arg)*));
    }}
}

pub fn create_log(filename: &str, file_type: &str) -> Mutex<std::fs::File> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();
    let logs_dir = format!("./logs/{}", timestamp);
    fs::create_dir_all(&logs_dir).expect("Failed to create logs directory");

    let file_path = format!("{}/{}.{}", logs_dir, filename, file_type);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .expect("Failed to open log file");

    Mutex::new(file)
}

// For now, we only have a log to log the time, sent packet and received response, formatted in a csv file.
lazy_static::lazy_static! {
    pub static ref EXECUTION_LOG: Mutex<std::fs::File> = {
        let file = create_log("execution", "csv");
        writeln!(file.lock().unwrap(), "time,sent_data,from_port,to_port,received_data,action").unwrap();
        file
    };
}

pub fn log(file: &Mutex<std::fs::File>, message: &str) {
    let mut file = file.lock().unwrap();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    writeln!(file, "{},{}", now, message).unwrap();
}
