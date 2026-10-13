//! Reads or sets a monitor DDC feature through strait's own DDC code.
//!
//!     cargo run -p strait-mac --example ddc -- 0x60          # read
//!     cargo run -p strait-mac --example ddc -- 0x60 15       # set

fn number(s: &str) -> u16 {
    match s.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16),
        None => s.parse(),
    }
    .unwrap_or_else(|_| panic!("not a number: {s}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let feature = number(args.first().expect("usage: ddc <feature> [value]")) as u8;
    let Some(mut monitor) = strait_mac::ddc::Monitor::find() else {
        eprintln!("no external display answers DDC");
        std::process::exit(1);
    };
    match args.get(1) {
        None => println!("{feature:#04x} = {:?}", monitor.read(feature)),
        Some(v) => println!("{feature:#04x} := {} -> {}", v, monitor.write(feature, number(v))),
    }
}
