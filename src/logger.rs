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

pub fn create_log(subfolder: &str, filename: &str, file_type: &str) -> Mutex<std::fs::File> {
    let logs_dir = format!("./logs/{}", subfolder);
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
        let now = Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let file = create_log(&timestamp, "execution", "csv");
        writeln!(file.lock().unwrap(), "time,sent_data,from_port,to_port,received_data,action").unwrap();
        file
    };
}

pub fn log(file: &Mutex<std::fs::File>, message: &str) {
    let mut file = file.lock().unwrap();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    writeln!(file, "{},{}", now, message).unwrap();
}

#[cfg(test)]
mod integration_tests_logger {
    use super::*;
    use std::fs;
    use std::io::Read;

    fn cleanup_log_file(path: &str) {
        fs::remove_dir_all(path).expect("Failed to remove log file");
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn test_create_log() {
        let now = Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string() + "_1";
        let filename = "test_create_log";
        let file_type = "log";
        let _ = create_log(&timestamp, filename, file_type);

        let logs_dir = format!("./logs/{}", timestamp);
        let file_path = format!("{}/{}.{}", logs_dir, filename, file_type);

        assert!(
            fs::metadata(&file_path).is_ok(),
            "File was not created or not on the right path: {}",
            &file_path
        );
        cleanup_log_file(&logs_dir);
    }

    #[test]
    // #[coverage(off)]  // Only available in nightly build, don't forget to uncomment #![feature(coverage_attribute)] on line 1 of main
    fn test_log_macro() {
        let now = Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S").to_string() + "_2";
        let filename = "test_macro";
        let file_type = "log";
        let log_file = create_log(&timestamp, filename, file_type);
        let message = "Test message";

        log!(log_file, "{}", message);

        let logs_dir = format!("./logs/{}", timestamp);
        let file_path = format!("{}/{}.{}", logs_dir, filename, file_type);

        let mut file = fs::File::open(&file_path).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();

        assert!(
            fs::metadata(&file_path).is_ok(),
            "File was not created or not on the right path: {}",
            &file_path
        );
        assert!(
            contents.contains(message),
            "Message was not logged correctly"
        );
        cleanup_log_file(&logs_dir);
    }
}
