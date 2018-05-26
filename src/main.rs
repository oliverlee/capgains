extern crate csv;

use std::env;
use std::error::Error;
use std::fs::File;
use std::process;

fn run(filename: &String) -> Result<(), Box<Error>> {
    let file = File::open(filename)?;
    let mut rdr = csv::Reader::from_reader(file);

    for result in rdr.records() {
        let record = result?;
        println!("{:?}", record);
    }

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Please supply a csv file with a transaction history.");
        process::exit(1);
    }

    let filename = &args[1];

    println!("Reading file {}", filename);

    if let Err(err) = run(filename) {
        println!("{}", err);
        process::exit(1);
    }
}
