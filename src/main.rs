mod mdr;

fn main() {
    println!("{}", banner());
}

fn banner() -> String {
    format!("xm-control {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn banner_has_version() {
        assert_eq!(super::banner(), concat!("xm-control ", env!("CARGO_PKG_VERSION")));
    }
}
