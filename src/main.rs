extern crate chrono;
extern crate csv;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::error;
use std::fmt;
use std::process;
use std::str::FromStr;

use serde::{de, Deserialize, Deserializer};

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug)]
struct SellRecord {
    amount: f64,
    cap_gains: f64,
    cap_gains_ratio: f64,
}

#[derive(Debug, Clone)]
struct AccountError(String);

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl error::Error for AccountError {
    fn description(&self) -> &str {
        &self.0
    }
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

    fn make_sell_records(
        &self,
        fund_prices: &HashMap<String, f64>,
    ) -> Result<Vec<SellRecord>, AccountError> {
        for fund in self.funds.iter() {
            if !fund_prices.contains_key(fund) {
                let s = format!("Missing price for fund: {}", fund);
                return Err(AccountError(s));
            }
        }

        let mut vec = Vec::new();
        for record in &self.records {
            let price = fund_prices.get(&record.fund).unwrap();
            let amount = price*record.num_shares;
            let cap_gains = (price - record.share_price)*record.num_shares;
            let cap_gains_ratio = cap_gains/amount;

            vec.push(SellRecord { amount, cap_gains, cap_gains_ratio } );
        }

        Ok(vec)
    }

    fn minimum_cap_gains(
        &self,
        fund_prices: &HashMap<String, f64>,
        sell_target: f64,
    ) -> Result<Vec<(Record, SellRecord)>, AccountError> {
        let sell_records = self.make_sell_records(fund_prices)?;

        let mut indices = Vec::new();
        for (i, item) in sell_records.iter().enumerate() {
            indices.push((item.cap_gains_ratio, i));
        }
        indices.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let mut amount = 0.0;
        let mut result = Vec::new();
        for (_, i) in indices {
            result.push((self.records[i].clone(), sell_records[i].clone()));
            amount += sell_records[i].amount;
            if amount > sell_target {
                break;
            }
        }

        if amount < sell_target {
            return Err(AccountError(format!("Insufficient funds.")))
        }

        Ok(result)
    }
}

fn load_account(filename: &String) -> Result<Account, Box<error::Error>> {
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

fn print_sell_summary(mut summary: Vec<(Record, SellRecord)>) {
    summary.sort_unstable_by(|a, b| {
        let date_a = chrono::NaiveDate::parse_from_str(&a.0.date, "%m/%d/%Y").unwrap();
        let date_b = chrono::NaiveDate::parse_from_str(&b.0.date, "%m/%d/%Y").unwrap();
        date_b.cmp(&date_a)
    });

    println!("Selling the following records:");

    let mut amount = 0.0;
    let mut cap_gains = 0.0;
    for (record, sell_record) in summary {
        println!("  {}, {}, {} shares", record.date, record.fund, record.num_shares);
        amount += sell_record.amount;
        cap_gains += sell_record.cap_gains;
    }

    println!("will result in");
    println!("amount: {:.2}", amount);
    println!("cap gains: {:.2}", cap_gains);
}

fn run(filename: &String, target_amount: f64) {
    let account = load_account(filename).unwrap();

    //println!("Got account with funds:");
    //for fund in account.funds.iter() {
    //    println!("{}", fund);
    //}

    // hardcode the fund prices for now
    let mut fund_prices: HashMap<String, f64> = HashMap::new();
    fund_prices.insert(
        "Total Stock Mkt Idx Adm".to_string(),
        68.44);
    fund_prices.insert(
        "Tot Intl Stock Ix Admiral".to_string(),
        30.31);

    //let sell_records = account.make_sell_records(&fund_prices).unwrap();
    //for record in &sell_records {
    //    println!("{:?}", record);
    //}
    let result = account.minimum_cap_gains(&fund_prices, target_amount).unwrap();
    print_sell_summary(result);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Please supply a csv file with a transaction history and a target sell amount.");
        process::exit(1);
    }

    let filename = &args[1];
    let target_amount = f64::from_str(&args[2]).unwrap();
    println!("Reading file {} with a target sell amount of {}", filename, target_amount);

    run(filename, target_amount);
    process::exit(0);
}
