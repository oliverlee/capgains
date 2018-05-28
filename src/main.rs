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
    date_purchased: chrono::NaiveDate,
    fund: String,
    num_shares: f64,
    share_price_purchased: f64,
    share_price: f64,
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
            let date_purchased = match chrono::NaiveDate::parse_from_str(&record.date, "%m/%d/%Y") {
                Ok(date) => date,
                Err(e) => return Err(AccountError(error::Error::description(&e).to_string())),
            };
            let fund = record.fund.clone(); // TODO use reference, lifetimes
            let num_shares = record.num_shares;
            let share_price_purchased = record.share_price;
            let share_price = *fund_prices.get(&record.fund).unwrap();
            let amount = share_price * num_shares;
            let cap_gains = (share_price - share_price_purchased) * num_shares;
            let cap_gains_ratio = cap_gains / amount;

            vec.push(SellRecord {
                date_purchased,
                fund,
                num_shares,
                share_price_purchased,
                share_price,
                amount,
                cap_gains,
                cap_gains_ratio,
            });
        }

        Ok(vec)
    }

    fn minimum_cap_gains(
        &self,
        fund_prices: &HashMap<String, f64>,
        sell_target: f64,
        tax_rate: f64,
    ) -> Result<Vec<SellRecord>, AccountError> {
        let mut sell_records = self.make_sell_records(fund_prices)?;

        let mut indices = Vec::new();
        for (i, item) in sell_records.iter().enumerate() {
            indices.push((item.cap_gains_ratio, i));
        }
        indices.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let mut sell_records: Vec<Option<SellRecord>> =
            sell_records.drain(..)
                        .map(|s| Some(s))
                        .collect();

        let mut amount = 0.0;
        let mut cap_gains = 0.0;
        let mut result = Vec::new();
        for (_, i) in indices {
            sell_records.push(None);
            let srec = sell_records.swap_remove(i).unwrap();
            amount += srec.amount;
            cap_gains += srec.cap_gains;

            // TODO: handle negative cap gains
            if (amount - cap_gains * tax_rate) > sell_target {
                // see if we can sell some (not all) of the shares of this record
                let mut x = srec.amount - srec.cap_gains * tax_rate;
                x /= srec.num_shares;

                // get pre-record values for amount and cap gains
                let a = amount - srec.amount;
                let c = cap_gains - srec.cap_gains;

                // get number of shares needed to reach sell target
                // shares can only be sold as integer amounts
                let n = ((sell_target - (a - c * tax_rate)) / x).trunc() + 1.0;

                if n < srec.num_shares.trunc() {
                    result.push(
                        SellRecord {
                            num_shares: n,
                            amount: srec.share_price*n,
                            cap_gains: (srec.share_price - srec.share_price_purchased)*n,
                            ..srec
                        }
                    );
                } else {
                    result.push(srec);
                }
                break;
            } else {
                result.push(srec);
            }
        }

        if amount < sell_target {
            return Err(AccountError("Insufficient funds.".to_string()));
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

fn print_sell_summary(mut summary: Vec<SellRecord>, tax_rate: f64) {
    summary.sort_unstable_by(|a, b| b.date_purchased.cmp(&a.date_purchased));
    println!("Selling the following records:");

    let mut amount = 0.0;
    let mut cap_gains = 0.0;
    for srec in summary {
        println!(
            "  {},\t{},\t{} shares",
            srec.date_purchased, srec.fund, srec.num_shares
        );
        amount += srec.amount;
        cap_gains += srec.cap_gains;
    }

    println!("will result in");
    println!("amount: {:.2}", amount);
    println!("cap gains: {:.2}", cap_gains);
    if tax_rate != 0.0 {
        println!("taxes: {:.2}", cap_gains * tax_rate);
        println!("net amount: {:.2}", amount - cap_gains * tax_rate);
    }
}

fn run(filename: &String, target_amount: f64, tax_rate: f64) {
    let account = load_account(filename).unwrap();

    //println!("Got account with funds:");
    //for fund in account.funds.iter() {
    //    println!("{}", fund);
    //}

    // hardcode the fund prices for now
    let mut fund_prices: HashMap<String, f64> = HashMap::new();
    fund_prices.insert("Total Stock Mkt Idx Adm".to_string(), 68.44);
    fund_prices.insert("Tot Intl Stock Ix Admiral".to_string(), 30.31);

    //let sell_records = account.make_sell_records(&fund_prices).unwrap();
    //for record in &sell_records {
    //    println!("{:?}", record);
    //}
    let result = account
        .minimum_cap_gains(&fund_prices, target_amount, tax_rate)
        .unwrap();
    print_sell_summary(result, tax_rate);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Please supply a csv file with a transaction history and a target sell amount.");
        process::exit(1);
    }

    let filename = &args[1];
    let target_amount = f64::from_str(&args[2]).unwrap();
    let mut tax_rate = 0.0;

    if args.len() > 3 {
        tax_rate = f64::from_str(&args[3]).unwrap();
        println!("using a tax rate of {}", tax_rate);
    }

    println!(
        "Reading file {} with a target sell amount of {}",
        filename, target_amount
    );

    run(filename, target_amount, tax_rate);
    process::exit(0);
}
