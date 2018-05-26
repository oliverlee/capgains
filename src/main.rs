extern crate csv;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::env;
use std::error::Error;
use std::process;
use std::collections::HashSet;

use serde::{de, Deserialize, Deserializer};
use std::str::FromStr;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Record {
    #[serde(rename = "Date")]
    date: String,
    #[serde(rename = "Fund")]
    fund: String,
    #[serde(rename = "Transaction type")]
    transaction_type: String,
    #[serde(rename = "Shares transacted")]
    num_shares: f64,
    #[serde(rename = "Share price", deserialize_with = "de_from_str")]
    share_price: f64,
    #[serde(rename = "Amount", deserialize_with = "de_from_str")]
    amount: f64, // dependent field
}

fn de_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    let clean_s = str::replace(s.trim_matches('$'), ",", "");
    f64::from_str(&clean_s).map_err(de::Error::custom)
}


struct Account {
    records: Vec<Record>,
    funds: HashSet<String>,
}

impl Account {
    fn new(records: Vec<Record>) -> Self {
        let mut funds = HashSet::new();
        for record in &records {
            funds.insert(record.fund.clone());
        }

        Account { records, funds }
    }
}


fn load_account(filename: &String) -> Result<Account, Box<Error>> {
    let mut rdr = csv::Reader::from_path(filename)?;
    let mut vec = Vec::new();
    let mut error: Option<csv::Error> = None;

    for result in rdr.deserialize::<Record>() {
        match result {
            Ok(record) => {
                match error {
                    Some(error) => return Err(Box::new(error)),
                    None => vec.push(record),
                };
            }
            Err(e) => error = Some(e),
        };
    }

    Ok(Account::new(vec))
}

fn run(filename: &String) {
    let account = load_account(filename).unwrap();

    println!("Got account with funds:");
    for fund in account.funds.iter() {
        println!("{}", fund);
    }
}


fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Please supply a csv file with a transaction history.");
        process::exit(1);
    }

    let filename = &args[1];
    println!("Reading file {}", filename);
    run(filename);
    process::exit(0);
}
