#![feature(map_get_key_value)]

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
struct Record {
    #[serde(rename = "Date", deserialize_with = "de_date_from_str")]
    date: chrono::NaiveDate,
    #[serde(rename = "Fund")]
    fund: String,
    #[serde(rename = "Transaction type")]
    transaction_type: String,
    #[serde(rename = "Shares transacted")]
    num_shares: f64,
    #[serde(rename = "Share price", deserialize_with = "de_usd_from_str")]
    share_price: f64,
    #[serde(rename = "Amount", deserialize_with = "de_usd_from_str")]
    amount: f64, // dependent field
}

#[derive(Clone, Debug)]
struct SellRecord<'a> {
    date_purchased: chrono::NaiveDate,
    fund: &'a str,
    num_shares: f64,
    share_price_purchased: f64,
    share_price: f64,
    amount: f64,
    cap_gains: f64,
    cap_gains_ratio: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct FundPrice {
    #[serde(rename = "Fund")]
    fund: String,
    #[serde(rename = "Share price", deserialize_with = "de_usd_from_str")]
    share_price: f64,
}

#[derive(Clone, Debug)]
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

fn de_date_from_str<'de, D>(deserializer: D) -> Result<chrono::NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    chrono::NaiveDate::parse_from_str(s, "%m/%d/%Y").map_err(de::Error::custom)
}

fn de_usd_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
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
            if !funds.contains(&record.fund) {
                funds.insert(record.fund.clone());
            }
        }

        Account { records, funds }
    }

    fn make_sell_records<'a>(
        &self,
        fund_prices: &'a HashMap<String, f64>,
    ) -> Result<Vec<SellRecord<'a>>, AccountError> {
        for fund in self.funds.iter() {
            if !fund_prices.contains_key(fund) {
                let s = format!("Missing price for fund: {}", fund);
                return Err(AccountError(s));
            }
        }

        let mut vec = Vec::new();
        for record in &self.records {
            let date_purchased = record.date;
            let fund = fund_prices.get_key_value(&record.fund).unwrap().0;
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

    fn minimum_cap_gains<'a>(
        &self,
        fund_prices: &'a HashMap<String, f64>,
        sell_target: f64,
        tax_rate: f64,
    ) -> Result<Vec<SellRecord<'a>>, AccountError> {
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

fn load_account(filename: &str) -> Result<Account, csv::Error> {
    let mut rdr = csv::Reader::from_path(filename)?;
    let mut vec = Vec::new();
    let mut error: Option<csv::Error> = None;

    for result in rdr.deserialize::<Record>() {
        match result {
            Ok(record) => {
                match error {
                    Some(error) => return Err(error),
                    None => vec.push(record),
                };
            }
            Err(err) => error = Some(err),
        };
    }

    Ok(Account::new(vec))
}

fn load_fund_prices(filename: &str) -> Result<HashMap<String, f64>, csv::Error> {
    let mut fund_prices: HashMap<String, f64> = HashMap::new();
    let mut rdr = csv::Reader::from_path(filename)?;

    for result in rdr.deserialize::<FundPrice>() {
        match result {
            Ok(fp) => fund_prices.insert(fp.fund, fp.share_price),
            Err(err) => return Err(err),
        };
    }

    Ok(fund_prices)
}

fn print_sell_summary(mut summary: Vec<SellRecord>, tax_rate: f64) {
    summary.sort_unstable_by(|a, b| b.date_purchased.cmp(&a.date_purchased));
    println!("Selling the following records:");

    let mut amount = 0.0;
    let mut cap_gains = 0.0;
    println!("  {:>10}, {:>25}, {:>10}, {:>10}, {:>10}, {:>10}", "date", "fund", "amount", "cap gains", "cg ratio", "shares");
    for srec in summary {
        // print out when selling a whole number of shares as it's not too common
        let shares = if srec.num_shares.fract() == 0.0 {
            format!("{:>10} [whole]", srec.num_shares)
        } else {
            format!("{:10.3}", srec.num_shares)
        };
        println!(
            "  {}, {:>25}, {:10.3}, {:10.3}, {:10.3}, {}",
            srec.date_purchased, srec.fund, srec.amount, srec.cap_gains, srec.cap_gains_ratio, shares
        );
        amount += srec.amount;
        cap_gains += srec.cap_gains;
    }

    println!("will result in");
    println!("amount:     {:10.3}", amount);
    println!("cap gains:  {:10.3}", cap_gains);
    if tax_rate != 0.0 {
        println!("taxes:      {:10.3}", cap_gains * tax_rate);
        println!("net amount: {:10.3}", amount - cap_gains * tax_rate);
    }
}

fn run(account_filename: &str, fundprice_filename: &str, sell_target: f64, tax_rate: f64) {
    let account = load_account(account_filename).unwrap();
    let fund_prices = load_fund_prices(fundprice_filename).unwrap();

    let result = account.minimum_cap_gains(&fund_prices, sell_target, tax_rate).unwrap();
    print_sell_summary(result, tax_rate);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        println!("Calculate the records to sell to minimize capital gains.");
        println!("usage: ./capgains <account_file> <fundprice_file> sell_target [tax_rate]");
        println!("\naccount_file: csv file with the following fields -- Date,Fund,Transaction type,Shares transacted,Share price,Amount");
        println!("fundprice_file: csv file with the following fields -- Fund,Share price");
        println!("sell_target: Target amount to sell.");
        println!("tax_rate: A flat tax rate to apply to capital gains. Taxes will be accounted for when selecting records to sell.");
        println!("");
        process::exit(1);
    }

    let account_filename = &args[1];
    let fundprice_filename = &args[2];
    let sell_target = f64::from_str(&args[3]).unwrap();

    println!("Reading account information from: {}", account_filename);
    println!("Reading fund price from: {}", fundprice_filename);
    println!("Minimizing capital gains for target sell amount of: {}", sell_target);

    let mut tax_rate = 0.0;
    if args.len() > 4 {
        tax_rate = f64::from_str(&args[4]).unwrap();
        println!("Applying a tax rate of {}%", 100.0*tax_rate);
    }
    println!("");

    run(account_filename, fundprice_filename, sell_target, tax_rate);
    process::exit(0);
}
