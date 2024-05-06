pub fn add(x: i32, y: i32) -> i32 {
    x + y
}


#[cfg(test)]
mod tests {
    #[test]
    fn t1() {
        assert_eq!(42, 42);
    }

    #[test]
    fn t2() {
        assert_eq!(crate::add(1, 2), 3);
    }
}
