// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: cloud_subnet_xml SUBNET_UUID RFC1918_CIDR");
        std::process::exit(2);
    }
    match machina_spec::cloud_network_xml(&args[1], &args[2]) {
        Ok(xml) => println!("{xml}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
