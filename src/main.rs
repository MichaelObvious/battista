use core::fmt;
use std::{
    cmp::Ordering, collections::{BTreeMap, HashMap, HashSet}, env, f64::consts::PI, fmt::Debug, fs, io::{self, Write}, path::PathBuf, process::{Command, exit}
};

use chrono::{Datelike, IsoWeek, Local, NaiveDate, TimeDelta, Weekday};
use quick_xml::{Reader, events::Event};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use rust_decimal_macros::dec;

const LAST_N_DAYS: [u64; 5] = [7, 14, 30, 90, 365];

type Category = String;
type Money = Decimal;

#[derive(Debug, Default, Clone)]
struct Transaction {
    value: Money,
    date: NaiveDate,
    category: Category,
    payment_method: String,
    note: String,
}

#[derive(Debug, Clone)]
enum DBEntry {
    Budget{
        category: Option<String>,
        amount: String,
        duration: String,
        date: String,
    },
    Transaction{
        amount: String,
        category: String,
        date: String,
        payment_method: String,
        note: String,
    },
    Extra{
        amount: String,
        date: String,
        payment_method: String,
        note: String,
    }
}


#[derive(Debug, Clone)]
struct RawTransaction {
    amount: String,
    category: String,
    date: String,
    payment_method: String,
    note: String,
}


impl fmt::Display for RawTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[TRANSACTION; {}; {}; {}; {}; `{}`]", self.date, self.category, self.amount, self.payment_method, self.note)    
    }
}

impl From<DBEntry> for RawTransaction {
    fn from(value: DBEntry) -> Self {
        if let DBEntry::Transaction { amount, category, date, payment_method, note } = value {
            RawTransaction{amount, category, date, payment_method, note}
        } else {
            panic!("Error converting to `RawTransaction`: Expected `DBEntry::Transaction`.");
        }
    }
}

impl From<RawTransaction> for DBEntry {
    fn from(value: RawTransaction) -> Self {
        DBEntry::Transaction{amount: value.amount, category: value.category, date: value.date, payment_method: value.payment_method, note: value.note}
    }
}
#[derive(Debug, Clone, Copy)]
enum Rate {
    Absolute(Money),
    Percentage(Decimal),
}

impl Rate {
    fn from_period(total: Money, days: Decimal) -> Self {
        Rate::Absolute((total / days).round_dp(2))
    }

    fn percentage(percentage: Decimal) -> Self {
        Rate::Percentage(percentage/dec!(100))
    }

    fn resolve(&self, general: Money) -> Money {
        match self {
            Rate::Absolute(m) => *m,
            Rate::Percentage(f) => (general * f).round_dp(2),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Schedule {
    changes: BTreeMap<NaiveDate, Rate>,
}

impl Schedule {
    fn set(&mut self, date: NaiveDate, rate: Rate) {
        self.changes.insert(date, rate);
    }

    fn at(&self, date: NaiveDate) -> Option<Rate> {
        self.changes.range(..=date).next_back().map(|(_, &r)| r)
    }
}

#[derive(Debug)]
enum BudgetError {
    CategoriesExceedGeneral {
        date: NaiveDate,
        category_sum: Money,
        general: Money,
    },
}

#[derive(Debug, Default, Clone)]
struct BudgetTimeline {
    general: BTreeMap<NaiveDate, Money>, // always absolute; no Rate
    categories: HashMap<Category, Schedule>,
    extras: BTreeMap<NaiveDate, Money>,  // one-off additions to the general on a specific date
}

impl BudgetTimeline {

    fn set_general(&mut self, date: NaiveDate, daily_rate: Money, days: Decimal) {
        self.general.insert(date, (daily_rate / days).round_dp(2));
    }

    fn set_category(&mut self, category: &Category, date: NaiveDate, rate: Rate) {
        self.categories
            .entry(category.into())
            .or_default()
            .set(date, rate);
    }

    fn add_extra(&mut self, date: NaiveDate, amount: Money) {
        *self.extras.entry(date).or_insert(Money::ZERO) += amount;
    }

    fn general_at(&self, date: NaiveDate) -> Money {
        let base = self.general
            .range(..=date)
            .next_back()
            .map(|(_, &m)| m)
            .unwrap_or(Money::ZERO);
        let extra = self.extras.get(&date).copied().unwrap_or(Money::ZERO);
        base + extra
    }

    fn category_at(&self, category: &str, date: NaiveDate) -> Money {
        let general = self.general_at(date);
        self.categories
            .get(category)
            .and_then(|s| s.at(date))
            .map(|r| r.resolve(general))
            .unwrap_or(Money::ZERO)
    }

    fn category_sum_at(&self, date: NaiveDate) -> Money {
        let general = self.general_at(date);
        self.categories
            .values()
            .filter_map(|s| s.at(date))
            .map(|r| r.resolve(general))
            .sum()
    }

    fn validate_at(&self, date: NaiveDate) -> Result<(), BudgetError> {
        let general = self.general_at(date);
        let sum = self.category_sum_at(date);
        if sum > general {
            Err(BudgetError::CategoriesExceedGeneral {
                date,
                category_sum: sum,
                general,
            })
        } else {
            Ok(())
        }
    }

    fn validate(&self) -> Result<(), Vec<BudgetError>> {
        let dates = self.general.keys()
            .chain(self.extras.keys())
            .chain(self.categories.keys().map(|x| self.categories[x].changes.keys()).flatten())
            .collect::<HashSet<_>>();
        
        let errors: Vec<BudgetError> = dates
        .iter()
        .filter_map(|&d| self.validate_at(*d).err())
        .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn accumulated_general(&self, start: NaiveDate, end: NaiveDate) -> Money {
        iter_days(start, end).map(|d| self.general_at(d)).sum()
    }

    fn accumulated_category(
        &self,
        category: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Money {
        iter_days(start, end).map(|d| self.category_at(category, d)).sum()
    }
}

fn iter_days(start: NaiveDate, end: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    let mut cursor = start;
    std::iter::from_fn(move || {
        if cursor <= end {
            let d = cursor;
            cursor += TimeDelta::days(1);
            Some(d)
        } else {
            None
        }
    })
}

#[derive(Debug, Default)]
struct Stats {
    start: NaiveDate,
    #[allow(unused)]
    end: NaiveDate,
    #[allow(unused)]
    per_day: Money,
    total: Money,
    by_category: Vec<(Category, Money)>,
    #[allow(unused)]
    by_payment_method: Vec<(String, Money)>,
    #[allow(unused)]
    by_note: Vec<(String, Money)>,
    by_day_of_week: Vec<(Weekday, Money)>,
    #[allow(unused)]
    average_transaction: Money,
    #[allow(unused)]
    transaction_count: u64,
}

#[derive(Debug, Default)]
struct TempStats {
    start: NaiveDate,
    end: NaiveDate,
    per_day: Money,
    total: Money,
    by_category: HashMap<Category, Money>,
    by_payment_method: HashMap<String, Money>,
    by_note: HashMap<String, Money>,
    by_day_of_week: HashMap<Weekday, Money>,
    average_transaction: Money,
    transaction_count: u64,
}

impl TempStats {
    fn update(&mut self, e: &Transaction) {
        let value = e.value;
        self.total += value;
        if !self.by_category.contains_key(&e.category) {
            self.by_category.insert(e.category.clone(), Money::ZERO);
        }
        *(self.by_category.get_mut(&e.category).unwrap()) += value;

        if !self.by_payment_method.contains_key(&e.payment_method) {
            self.by_payment_method.insert(e.payment_method.clone(), Money::ZERO);
        }
        *(self.by_payment_method.get_mut(&e.payment_method).unwrap()) += value;

        if !self.by_note.contains_key(&e.note) {
            self.by_note.insert(e.note.clone(), Money::ZERO);
        }
        *(self.by_note.get_mut(&e.note).unwrap()) += value;

        self.by_day_of_week.entry(e.date.weekday()).and_modify(|curr| *curr += value).or_insert(value);

        if self.start == NaiveDate::default() {
            self.start = e.date;
        } else {
            self.start = self.start.min(e.date);
        }
        if self.end == NaiveDate::default() {
            self.end = e.date;
        } else {
            self.end = self.end.max(e.date);
        }

        self.transaction_count += 1;
    }

    fn calc_averages(&mut self, days: u64) {
        let days: Money = days.into();
        self.per_day = self.total / days;
        if self.transaction_count != 0 {
            self.average_transaction = self.total / Decimal::from(self.transaction_count);
        } else {
            assert!(self.total == dec!(0));
            self.average_transaction = self.total;
        }
    }

    fn into_stats(mut self) -> Stats {
        let mut by_category = self.by_category.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_category.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_payment_method = self.by_payment_method.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_payment_method.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_note = self.by_note.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_note.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        for d in 0..7 {
            let wd = Weekday::from_i32(d).unwrap();
            self.by_day_of_week.entry(wd).or_insert(Money::ZERO);
        }
        let mut by_day_of_week = self.by_day_of_week.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_day_of_week.sort_by(|x, y| (x.0.num_days_from_monday() % 7).cmp(&(y.0.num_days_from_monday() % 7)));


        Stats {
            start: self.start,
            end: self.end,
            per_day: self.per_day,
            total: self.total,
            by_category,
            by_payment_method,
            by_note,
            by_day_of_week,
            average_transaction: self.average_transaction,
            transaction_count: self.transaction_count,
        }
    }
}

#[derive(Debug, Default)]
struct StatsCollection {
    #[allow(unused)]
    start: NaiveDate,
    #[allow(unused)]
    end: NaiveDate,
    yearly: Vec<(i32, Stats)>,         // year
    monthly: Vec<((i32, u32), Stats)>, // year, month
    weekly: Vec<(IsoWeek, Stats)>,     // year, week
    last_n_days: HashMap<u64, Stats>,
    transactions: Vec<Transaction>,
}

#[derive(Debug, Default)]
struct TempStatsCollection {
    start: NaiveDate,
    end: NaiveDate,
    yearly: HashMap<i32, TempStats>,         // year
    monthly: HashMap<(i32, u32), TempStats>, // year, month
    weekly: HashMap<IsoWeek, TempStats>,     // year, week
    last_n_days: HashMap<u64, TempStats>,
    transactions: Vec<Transaction>
}

impl TempStatsCollection {
    fn into_stats_collection(self) -> StatsCollection {
        let mut yearly = self
            .yearly
            .into_iter()
            .map(|(a, b)| (a, b.into_stats()))
            .collect::<Vec<_>>();
        yearly.sort_by(|x, y| x.0.cmp(&y.0));
        let mut monthly = self
            .monthly
            .into_iter()
            .map(|(a, b)| (a, b.into_stats()))
            .collect::<Vec<_>>();
        monthly.sort_by(|x, y| (x.0 .0 * 12 + x.0 .1 as i32).cmp(&(y.0 .0 * 12 + y.0 .1 as i32)));
        let mut weekly = self
            .weekly
            .into_iter()
            .map(|(a, b)| (a, b.into_stats()))
            .collect::<Vec<_>>();
        weekly.sort_by(|x, y| (x.0).cmp(&y.0));
        StatsCollection {
            start: self.start,
            end: self.end,
            yearly: yearly,
            monthly: monthly,
            weekly: weekly,
            last_n_days: self.last_n_days.into_iter().map(|(x,y)| (x, y.into_stats())).collect(),
            transactions: self.transactions,
        }
    }
}

fn tbold(s: &str) -> String {
    format!("{}[1m {}{}[0m",0o033 as char, s,0o033 as char)
}
fn tclear() -> String {
    format!("{}[2J{}[0;0H{}[K",0o033 as char,0o033 as char,0o033 as char)
}

fn next_month(d: NaiveDate) -> NaiveDate {
    let year = d.year();
    let month = d.month0() + 1;
    NaiveDate::from_ymd_opt(year + if month == 12 { 1 } else { 0 }, (month % 12) + 1, 1).unwrap()
}

fn days_in_month(d: NaiveDate) -> i64 {
    let year = d.year();
    let month = d.month0() + 1;
    (next_month(d)
        - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days()
}

fn days_in_year(d: NaiveDate) -> i64 {
    let year = d.year();
    (NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
        - NaiveDate::from_ymd_opt(year, 1, 1).unwrap())
    .num_days()
}

fn validate_amount(amount: &str) -> bool {
    let amount_str = amount.trim();
    if amount_str.is_empty() {
        return false;
    }

    if let Ok(parsed) = amount_str.parse::<Money>() {
        parsed.fract().mantissa() < 100
    } else {
        false
    }
}

fn accumulated_overspending(transactions: &[Transaction], budget: &BudgetTimeline) -> Vec<Money> {
    if transactions.is_empty() {
        return Vec::new();
    }

    let first_date = transactions.iter().map(|t| t.date).min().unwrap();
    let last_date = Local::now().date_naive();

    let mut result: Vec<Decimal> = Vec::new();
    let mut accumulated = Money::ZERO;

    let per_day = {
        let mut per_day_map = HashMap::with_capacity((last_date - first_date).num_days() as usize + 7);
        for t in transactions {
            *per_day_map.entry(t.date).or_insert(Money::ZERO) += t.value;
        }
        per_day_map
    };

    for current in iter_days(first_date, last_date) {
        let daily_spending: Money = *per_day.get(&current).unwrap_or(&Money::ZERO);
        let daily_budget = budget.general_at(current);
        let overspending = daily_spending - daily_budget;
        accumulated += overspending;
        result.push(accumulated);
    }

    result
}

fn monthly_averages(today: NaiveDate, accumulated: &[Money]) -> Vec<(Money, usize, usize)> {
    if accumulated.is_empty() {
        return Vec::new();
    }

    let len = accumulated.len();
    let first_date = today - TimeDelta::days(len as i64 - 1);

    let mut result = Vec::new();
    let mut month_start_idx: usize = 0;

    let mut i = 0;
    while i <= len {
        let is_last = i == len;
        let new_month = !is_last && {
            let date = first_date + TimeDelta::days(i as i64);
            let prev_date = first_date + TimeDelta::days(month_start_idx as i64);
            (date.year(), date.month()) != (prev_date.year(), prev_date.month())
        };

        if is_last || new_month {
            let month_end_idx = i - 1; // inclusive

            let sum: Money = accumulated[month_start_idx..=month_end_idx]
                .iter()
                .copied()
                .sum();
            let count = month_end_idx - month_start_idx + 1;

            result.push((
                sum / Decimal::from(count as i64),
                len - 1 - month_start_idx,
                len - 1 - month_end_idx,
            ));

            month_start_idx = i;
        }

        i += 1;
    }

    result
}

fn recovery_days(overspent_total: Money, allowed_budget_fraction: Decimal, start_date: NaiveDate, budget: &BudgetTimeline) -> i64 {
    let mut overspent = overspent_total;
    let fraction = dec!(1.0) - allowed_budget_fraction;
    let mut days = 0;
    while days < 366*100 && overspent > dec!(0.0) {
        overspent -= budget.general_at(start_date + TimeDelta::days(days)) * fraction;
        days += 1;
    }
    return days;
}

fn is_recovery_getting_closer(overspent_history: &Vec<Money>, allowed_budget_fraction: Decimal, budget: &BudgetTimeline) -> f64 {
    let today = Local::now().date_naive();
    let window = 14.min(overspent_history.len() - 1) as i64;
    let mut total = 0;
    for i in (-window)..0 {
        let overspent_total = overspent_history[(overspent_history.len()as i64 + i - 1) as usize];
        let rd =  recovery_days(overspent_total, allowed_budget_fraction, today + TimeDelta::days(i), budget);
        total += rd + i;
    }
    let average = total as f64 / window as f64;
    let days = recovery_days(*overspent_history.last().unwrap(), allowed_budget_fraction, today, budget) as f64;
    return days - average;
}

fn parse_file(filepath: &PathBuf) -> (Vec<Transaction>, BudgetTimeline) {
    let content = fs::read_to_string(&filepath).unwrap_or_default();
    let mut transactions  = Vec::new();
    let mut budget = BudgetTimeline::default();

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match String::from_utf8(e.name().as_ref().to_vec()).unwrap().as_str() {
                "budget" => {
                    let attributes =  e.attributes().map(|x| {
                        let x = x.unwrap();
                        (String::from_utf8(x.key.as_ref().to_vec()).unwrap(), String::from_utf8(x.value.as_ref().to_vec()).unwrap())
                    }).collect::<HashMap<_,_>>();
                    let pot_category = attributes.get("category");
                    let date = NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap();
                    let amount_str = attributes.get("amount").unwrap().trim();
                    if let Some(category) = pot_category {
                        let rate = if amount_str.ends_with('%') {
                            let amount_str = amount_str.replace('%', "");
                            Rate::percentage(amount_str.parse::<Money>().unwrap())
                        } else {
                            Rate::from_period(amount_str.parse::<Money>().unwrap(), attributes.get("duration").unwrap().parse::<Decimal>().unwrap())
                        };
                        budget.set_category(category, date, rate);
                    } else {
                        if amount_str.ends_with('%') {
                            eprintln!("[ERROR] No percentage allowed in general budget: `{}`.\n        Either add category or insert an absolute amount.", String::from_utf8(e.to_vec()).unwrap());
                            exit(1);
                        }
                        budget.set_general(date, amount_str.parse::<Money>().unwrap(), attributes.get("duration").unwrap().parse::<Decimal>().unwrap());
                    }
                    
                },
                "transaction" => {
                    let attributes =  e.attributes().map(|x| {
                        let x = x.unwrap();
                        (String::from_utf8(x.key.as_ref().to_vec()).unwrap(), String::from_utf8(x.value.as_ref().to_vec()).unwrap())
                    }).collect::<HashMap<_,_>>();

                    // assert!(attributes.get("amount").unwrap().chars().skip_while(|c| *c != '.').take_while(|c| c.is_numeric()).collect::<Vec<_>>().len() <= 2);
                    transactions.push(Transaction {
                        category: attributes.get("category").unwrap_or(&String::default()).trim().to_owned(),
                        date: NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap(),
                        value: attributes.get("amount").unwrap().parse::<Money>().unwrap(),
                        note: if attributes.contains_key("note") { attributes.get("note").unwrap().to_owned() } else { String::default() },
                        payment_method: attributes.get("payment-method").unwrap().trim().to_owned(),
                    });
                },
                "extra" => {
                    let attributes =  e.attributes().map(|x| {
                        let x = x.unwrap();
                        (String::from_utf8(x.key.as_ref().to_vec()).unwrap(), String::from_utf8(x.value.as_ref().to_vec()).unwrap())
                    }).collect::<HashMap<_,_>>();

                    // assert!(attributes.get("amount").unwrap().chars().skip_while(|c| *c != '.').take_while(|c| c.is_numeric()).collect::<Vec<_>>().len() <= 2);
                    let date = NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap();
                    let value = attributes.get("amount").unwrap().parse::<Money>().unwrap();
                    budget.add_extra(date, value);
                },
                x => {
                    eprintln!("[ERROR]: Unknown tag: `{}`.", x);
                },
            },
            Ok(_) => {},
            Err(e) => {
                eprintln!("[ERROR]: XML parsing error: `{}`.", e);
                exit(1);
            }
        }
    }

    match budget.validate() {
        Err(errors) => {
            for e in errors {
                match e {
                    BudgetError::CategoriesExceedGeneral { date, category_sum, general } => {
                        eprintln!("[ERROR] The sum of the categories at date `{}` is greater than the general budget.\n        Sum of categories: {}\n        General budget value: {}", date.format("%d/%m/%Y"), category_sum, general);
                    }
                }
            }
            exit(1);
        },
        Ok(_) => {},
    }

    return (transactions, budget);
}

fn get_stats(transactions: &Vec<Transaction>) -> StatsCollection {
    let mut tsc = TempStatsCollection::default();
    tsc.transactions = transactions.clone();
    for n in LAST_N_DAYS {
        tsc.last_n_days.insert(n, TempStats::default());
    }
    let today = Local::now().date_naive();
    let mut first_date = today;

    let mut start = today;
    let mut end = NaiveDate::from_ymd_opt(1, 1, 1).unwrap();
    for transaction in transactions.iter() {
        first_date = first_date.min(transaction.date);

        let year = transaction.date.year();
        let month = transaction.date.month0() + 1;
        let week = transaction.date.iso_week();
        start = start.min(transaction.date);
        end = end.max(transaction.date);

        if !tsc.yearly.contains_key(&year) {
            tsc.yearly.insert(year, TempStats::default());
        }
        tsc.yearly.get_mut(&year).unwrap().update(transaction);

        let month_idx = (year, month);
        if !tsc.monthly.contains_key(&month_idx) {
            tsc.monthly.insert(month_idx, TempStats::default());
        }
        tsc.monthly.get_mut(&month_idx).unwrap().update(transaction);
        if !tsc.weekly.contains_key(&week) {
            tsc.weekly.insert(week, TempStats::default());
        }

        tsc.weekly.get_mut(&week).unwrap().update(transaction);

        for i in LAST_N_DAYS.iter() {
            if (today - transaction.date).num_days() < *i as i64 {
                if !tsc.last_n_days.contains_key(i) {
                    tsc.last_n_days.insert(*i, TempStats::default());
                }
                tsc.last_n_days.get_mut(&i).unwrap().update(transaction);
            }
        }
    }

    tsc.start = start;
    tsc.end = end;

    {
        let mut current = first_date;
        while current <= today {
            let year = current.year();
            let month = current.month();
            let month_idx = (year, month);
            let week = current.iso_week();

            if !tsc.yearly.contains_key(&year) {
                tsc.yearly.insert(year, TempStats::default());
            }

            if !tsc.monthly.contains_key(&month_idx) {
                tsc.monthly.insert(month_idx, TempStats::default());
            }

            if !tsc.weekly.contains_key(&week) {
                tsc.weekly.insert(week, TempStats::default());
            }

            current += TimeDelta::days(1);
        }
    }

    for (k, v) in tsc.yearly.iter_mut() {
        let year_start = NaiveDate::from_ymd_opt(*k, 1, 1).unwrap();
        let period_start = year_start.max(start);
        let period_end = (NaiveDate::from_ymd_opt(*k + 1, 1, 1).unwrap() - TimeDelta::days(1))
            .min(today + TimeDelta::days(1));
        let days = days_in_year(year_start);
        let days2 = (period_end - period_start).num_days()+1;
        let ndays = days.min(days2);
        assert!(ndays > 0);
        v.calc_averages(ndays as u64);
    }

    for (k, v) in tsc.monthly.iter_mut() {
        let month_start = NaiveDate::from_ymd_opt(k.0, k.1, 1).unwrap();

        let month_end =
            NaiveDate::from_ymd_opt(k.0 + if k.1 == 12 { 1 } else { 0 }, (k.1 % 12) + 1, 1)
                .unwrap()
                - TimeDelta::days(1);
        let period_start = month_start.max(start);
        let period_end = (month_end + TimeDelta::days(1)).min(today + TimeDelta::days(1));
        let days = days_in_month(month_start);
        let days2 = (period_end - period_start).num_days()+1;
        let ndays = days.min(days2);
        assert!(ndays > 0);
        v.calc_averages(ndays as u64);
    }

    for (week, v) in tsc.weekly.iter_mut() {
        let week_start = NaiveDate::from_isoywd_opt(week.year(), week.week(), Weekday::Mon).unwrap();
        let week_end = today.min(week_start + TimeDelta::days(7));
        let ndays = (week_end - week_start).num_days()+1;
        assert!(ndays > 0);
        v.calc_averages(ndays as u64);
    }

    let ns = tsc.last_n_days.keys().map(|x| *x).collect::<Vec<_>>();
    for n in ns.into_iter() {
        tsc.last_n_days.get_mut(&n).unwrap().calc_averages(n);
    }

    return tsc.into_stats_collection();
}

fn percentage_to_color(x: Decimal) -> &'static str {
    if dec!(0.0) <= x && x < dec!(0.85) {
        return "green";
    } else if dec!(0.85) <= x && x <= dec!(1.00) {
        return "orange";
    } else if dec!(1.0) < x {
        return "red";
    } else {
        unreachable!()
    }
}

fn write_typ_table(buf: &mut Vec<u8>, stats: &StatsCollection, budget: &BudgetTimeline, n_days: u64) {
    let today = Local::now().date_naive();
    let stats = stats.last_n_days.get(&n_days).unwrap();
    writeln!(buf, "== Last {} days", n_days).unwrap();
        writeln!(buf, "").unwrap();
        writeln!(buf, "#align(center, table(columns: 4, align: left, stroke: 0pt, column-gutter: 5pt, table.hline(stroke: 1pt), [*Category*], [*Amount*], [*% of Budget*], [*Allowed spending*],").unwrap();
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        let n_days_accumulated =  budget.accumulated_general(today - TimeDelta::days(n_days as i64), today);
        for (category, amount) in stats.by_category.iter() {
            let allowed_amount = budget.accumulated_category(category, today - TimeDelta::days(n_days as i64), today);
            let allowed_amount = if allowed_amount > Money::ZERO {
                if n_days_accumulated > stats.total || allowed_amount - amount <= dec!(0.0) {
                    let allowed = allowed_amount - *amount;
                    (format!("{:.0}", allowed), format!("{:.0}%", (amount*dec!(100.0))/allowed_amount), if allowed >= dec!(0.0)  {
                        if allowed / allowed_amount >= dec!(0.15) {
                            "green"
                        } else {
                            "orange"
                        }
                    } else {
                        "red"
                    })
                } else {
                    let allowed = allowed_amount - *amount;
                    (String::default(), format!("{:.0}%", (amount*dec!(100.0))/allowed_amount), if allowed >= dec!(0.0)  {
                        if allowed / allowed_amount >= dec!(0.15) {
                            "green"
                        } else {
                            "orange"
                        }
                    } else {
                        "red"
                    })
                }
            } else {
                (String::default(), String::default(), "black")
            };
            write!(buf, "    [{}], align(right, [`{:.2}`]), ", category, amount).unwrap();
            write!(buf, "align(right, text([`{}`], fill: {})), ", allowed_amount.1, "black").unwrap();
            write!(buf, "align(right, text([`{}`], fill: {})), ", allowed_amount.0, allowed_amount.2).unwrap();
            writeln!(buf).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap()
        }
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        let allowed_amount = if budget.general_at(today) > dec!(0.0) {
                let total_allowed = n_days_accumulated;
                let allowed = total_allowed - stats.total;
                (format!("{:.0}", allowed), format!("{:.0}%", (stats.total*dec!(100.0))/total_allowed), if allowed >= dec!(0.0)  {
                    if allowed / total_allowed >= dec!(0.15) {
                        "green"
                    } else {
                        "orange"
                    }
                } else {
                    "red"
                })
            } else {
                (String::default(), String::default(), "black")
            };
        writeln!(buf, "    [*Total*], align(right, [`{:.2}`]), align(right, text([`{}`], fill: {})), align(right, text([`{}`], fill: {})), ", stats.total,  allowed_amount.1, allowed_amount.2, allowed_amount.0, allowed_amount.2).unwrap();
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        writeln!(buf, "))").unwrap();
        writeln!(buf, "").unwrap();
        if n_days >= 90 {
            writeln!(buf, "=== Biggest expenses (last {} days)", n_days).unwrap();

            let biggest_expenses = stats.by_note.iter().filter(|x| x.1/stats.total >= dec!(0.01) && x.1 >= dec!(50.0)).zip(0..20).collect::<Vec<_>>();
            // let max_expense = biggest_expenses.first().unwrap().0.1;
            writeln!(buf, "#align(center, table(columns: 3, stroke: 0pt, align: (right, left, right), ").unwrap();
            for ((note, amount),i) in biggest_expenses {
                // let percentage_box =  {
                //     let p = amount*dec!(100.0)/stats.total;
                //     if p < dec!(1.0) {
                //         String::default()
                //     } else {
                //         format!("#align(horizon, [#h(2.5em)] + box(fill: black.lighten({:.2}%), height: 0.67em, width: {:.2}em, stroke: 1pt + black) + [#h(0.5em) `{:.0}%`])", dec!(95)-dec!(10.0)*(amount/max_expense), dec!(5.0)*amount/max_expense, p.round())
                //     }
                // };
                writeln!(buf, "[#h(2.5em) {}.], [_\"{}\"_], [`{:.2}`], ", i+1, note, amount).unwrap();
            }
            writeln!(buf, "))").unwrap();
        }
}

fn write_typ_report(file_path: &PathBuf, stats: &StatsCollection, budget: &BudgetTimeline, original_path: &PathBuf) {
    let today = Local::now().date_naive();

    let mut buf = Vec::new();
    writeln!(buf, "#import \"@preview/cetz:0.4.2\"").unwrap();
    writeln!(buf, "#import \"@preview/cetz-plot:0.1.3\"").unwrap();
    writeln!(buf, "").unwrap();
    writeln!(buf, "#set document(title: [Spending report from {} ({})])", original_path.display(), today.format("%-d %B %Y")).unwrap();
    writeln!(buf, "#set page(width: 320mm, height: 200mm, numbering: \"1 of 1\")").unwrap();
    writeln!(buf, "#set text(12pt)").unwrap();
    writeln!(buf, "#show heading.where(level: 3): it => pagebreak() + align(center, box(inset: (top: 2em, bottom: 0.25em), text(it, 16pt)))").unwrap();
    writeln!(buf, "#show heading.where(level: 2): it => pagebreak() + align(center, box(inset: (top: 1em, bottom: 0.25em), text(it, 18pt)))").unwrap();
    writeln!(buf, "#show heading.where(level: 1): it => pagebreak() + align(center, box(inset: (top: 2em, bottom: 0.5em), text(it, 24pt)))").unwrap();
    writeln!(buf, "#set heading(numbering: \"1.\")").unwrap();
    writeln!(buf, "").unwrap();

    writeln!(buf, "#v(1fr)").unwrap();
    writeln!(buf, "#align(center, text([*Spending report from* `{}`], 28pt))", original_path.display()).unwrap();
    writeln!(buf, "#align(center, text([{}], 20pt))", today.format("%B %-d, %Y")).unwrap();
    writeln!(buf, "#v(1.25fr)").unwrap();
    writeln!(buf, "#align(center, link(\"https://www.github.com/MichaelObvious/battista\", text([`battista {}`], 16pt)))", env!("CARGO_PKG_VERSION")).unwrap();
    writeln!(buf, "").unwrap();
    writeln!(buf, "#v(3em)").unwrap();
    writeln!(buf, "").unwrap();
    writeln!(buf, "#pagebreak(weak: true)").unwrap();

    writeln!(buf, "").unwrap();
    writeln!(buf, "#outline()").unwrap();
    writeln!(buf, "").unwrap();

    writeln!(buf, "= Budget").unwrap();
    writeln!(buf, "#v(1fr)").unwrap();
    writeln!(buf, "#columns(2, [").unwrap();
    writeln!(buf, "#align(center, text([*Next Month's Budget*], 18pt)) ").unwrap();
    writeln!(buf, "#align(center, table(columns: 3, stroke: 0pt, align: (left, right, right), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Category*], align(left, [*Allowed monthly amount*]), align(left, [*% of Total*]), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    let monthly_total_budget = budget.accumulated_general(today, today+TimeDelta::days(30));
    let mut budget_categories = budget.categories.iter().map(|(c,_)| (c, budget.accumulated_category(c, today, today + TimeDelta::days(30)))).collect::<Vec<_>>();
    budget_categories.sort_by_key(|(_,b)| -b);
    let mut total_allocated = dec!(0.0);
    for (category, monthly_budget) in budget_categories.iter() {
        total_allocated += monthly_budget;
        if *monthly_budget > dec!(0.00) && (monthly_budget / monthly_total_budget)*dec!(100.0) >= dec!(1.0) {
            writeln!(buf, "[{}], [`{:.0}`], [`{:.0}%`],", category, monthly_budget, ((monthly_budget / monthly_total_budget)*dec!(100.0)).round()).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
        }
    }
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[_Unallocated_], [_`{:.0}`_], [_`{:.0}%`_],", monthly_total_budget - total_allocated, (dec!(100.0) - total_allocated * dec!(100.0) / monthly_total_budget).round()).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Total*], [`{:.0}`], ", monthly_total_budget).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "))").unwrap();

    let min_budget = (monthly_total_budget
    .min(budget.accumulated_general(today,today + TimeDelta::days(7)) * dec!(30) / dec!(7))
    .min(budget.general_at(today) * dec!(30)) / dec!(30)).round_dp(2);

writeln!(buf, "#colbreak()").unwrap();
    
    writeln!(buf, "#align(center, text([*Per Period*], 18pt)) ").unwrap();
    writeln!(buf, "#align(center, [_Conservative indicative values_]) ").unwrap();
    writeln!(buf, "#align(center, table(columns: 2, stroke: 0pt, align: (left, right), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Period*], align(left, [*Allowed amount*]), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "    [_Per month_], align(right, [`{:.0}`]),", min_budget * dec!(30)).unwrap();
    writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
    writeln!(buf, "    [_Per week_],  align(right, [`{:.0}`]),", min_budget * dec!(7)).unwrap();
    writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
    writeln!(buf, "    [_Per day_],   align(right, [`{:.0}`]),", min_budget).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "))").unwrap();
    writeln!(buf, "])").unwrap();
    writeln!(buf, "#v(1.5fr)").unwrap();
    
    writeln!(buf, "#pagebreak()").unwrap();

    writeln!(buf, "#v(1fr)").unwrap();
    writeln!(buf, "#align(center)[").unwrap();
    writeln!(buf, "  #text(18pt)[*Next Month's Budget*]").unwrap();
    writeln!(buf, "#v(2em)").unwrap();
    
    let mut total_allocated = dec!(0);
    let mut slices  = Vec::new();

    for (category, monthly_budget) in budget_categories.iter() {
        total_allocated += monthly_budget;
        if *monthly_budget > dec!(0) && (monthly_budget / monthly_total_budget) * dec!(100) >= dec!(1) {
            slices.push((category.to_owned().to_owned(), *monthly_budget));
        }
    }
    let unallocated = monthly_total_budget - total_allocated;
    if unallocated > dec!(0) {
        slices.push(("Unallocated".to_string(), unallocated));
    }

    writeln!(buf, "  #cetz.canvas({{").unwrap();
    writeln!(buf, "    import cetz.draw: *").unwrap();
    writeln!(buf, "import cetz-plot: *").unwrap();
    writeln!(buf, "    chart.piechart(").unwrap();
    writeln!(buf, "      (").unwrap();
    for (category, amount) in &slices {
        writeln!(buf, "        (\"{}\", {:.0}),", category.replace('"', "\\\""), amount).unwrap();
    }
    writeln!(buf, "      ),").unwrap();
    writeln!(buf, "      start: 90deg,").unwrap();
    writeln!(buf, "      stop: 450deg,").unwrap();
    writeln!(buf, "      gap: 1deg,").unwrap();
    writeln!(buf, "      value-key: 1,").unwrap();
    writeln!(buf, "      label-key: 0,").unwrap();
    writeln!(buf, "      legend: (label: none),").unwrap();
    writeln!(buf, "      radius: 4.25,").unwrap();
    writeln!(buf, "      inner-radius: 4.15,").unwrap();
    writeln!(buf, "      slice-style: (").unwrap();
    for (i,_) in slices.iter().enumerate() {
        let mut transparency = (i as f64 / (slices.len()-1) as f64).sqrt() * 100.0;
        if transparency.is_nan() {
            transparency = 0.0;
        }
        writeln!(buf, "        (fill: black.transparentize({}%), stroke: none),", transparency).unwrap();
    }
    writeln!(buf, "      ),").unwrap();
    writeln!(buf, "      inner-label: (content: \"%\", radius: 85%),").unwrap();
    writeln!(buf, "      outer-label: (").unwrap();
    writeln!(buf, "        content: (value, label) => {{").unwrap();
    writeln!(buf, "                align(center)[#label #linebreak() (#text(0.7em, font: \"DejaVu Sans Mono\", [#value])) ]").unwrap();
    writeln!(buf, "        }},").unwrap();
    writeln!(buf, "        radius: 135%,").unwrap();
    writeln!(buf, "      ),").unwrap();
    writeln!(buf, "    )").unwrap();
    writeln!(buf, "  }})").unwrap();
    writeln!(buf, "]").unwrap();
    writeln!(buf, "").unwrap();
    writeln!(buf, "#v(1fr)").unwrap();
    
    writeln!(buf, "").unwrap();
    const RECOVERY_PLAN_MIN_BUDGET_FRACTION: Money = dec!(0.55);
    
    let mut accumulated = accumulated_overspending(&stats.transactions, budget);
    let next_month_budget = monthly_total_budget;
    let mut allowed_next_month = next_month_budget + budget.accumulated_general(today - TimeDelta::days(30), today) - stats.last_n_days.get(&30).unwrap().total;
    let overspent_total = accumulated.last().unwrap().clone();
    let next_year_budget =  budget.accumulated_general(today, today + TimeDelta::days(365));
    let year_fraction = (dec!(1.0) - dec!(1.25) * (stats.last_n_days.get(&365).unwrap().total - next_year_budget)/next_year_budget).max(RECOVERY_PLAN_MIN_BUDGET_FRACTION / dec!(0.95)) * dec!(0.95);
    let month_fraction = allowed_next_month / next_month_budget * dec!(0.95);
    let fraction = year_fraction.min(month_fraction).min(dec!(0.8));
    let recover_time_days = (Decimal::from(recovery_days(overspent_total, fraction, today, budget)) * dec!(1.1)).ceil();
    writeln!(buf, "#pagebreak()").unwrap();
    writeln!(buf, "#v(1fr)").unwrap();
    {
        let color = if allowed_next_month < next_month_budget * dec!(0.67) { "red" } else if allowed_next_month < next_month_budget * dec!(0.85) { "orange" } else { "black" };
        if allowed_next_month < next_month_budget * dec!(0.75) {
            allowed_next_month = allowed_next_month.max(next_month_budget * RECOVERY_PLAN_MIN_BUDGET_FRACTION * (dec!(1.0)/dec!(0.95)));
        }
        if allowed_next_month < next_month_budget || *accumulated.last().unwrap() > dec!(0.0) {
            writeln!(buf, "#align(center, box(radius: 2em, stroke: 2pt + {}, inset: 2em, [", color).unwrap();
            writeln!(buf, "#align(center,text(fill: {color}, [You overspent in the last period.]) + [\\ For the next month, we suggest the following budget.])").unwrap();
            writeln!(buf, "#v(0.5em)").unwrap();
            writeln!(buf, "#align(center, table(columns: 2, stroke: 0pt, align: (left, right, right), ").unwrap();
            writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
            writeln!(buf, "    [*Period* #h(2em)], [*Allowed amount* (`{:.0}%` _of user budget_)],", fraction*dec!(100.0)).unwrap();
            writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
            writeln!(buf, "    [_Per month_], align(right, [`{:.0}`]),", fraction * min_budget * dec!(30)).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
            writeln!(buf, "    [_Per week_],  align(right, [`{:.0}`]),", fraction * min_budget * dec!(7.0)).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
            writeln!(buf, "    [_Per day_],   align(right, [`{:.0}`]),", fraction * min_budget).unwrap();
            writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
            writeln!(buf, "))").unwrap();
            if overspent_total > dec!(0.0) {
                let recover_date = today + TimeDelta::days(recover_time_days.ceil().trunc().as_i128() as i64);
                let recover_date_drift_symbol = {
                    let derivative = is_recovery_getting_closer(&accumulated, fraction, budget);
                    let angle = -(derivative.atan() * 2.0 / PI) * 45.0;
                    format!("#box(move(dy: 0.33em, scale(67%, rotate({}deg, sym.arrow))))", angle)
                };
                writeln!(buf, "").unwrap();
                writeln!(buf, "#v(1em)").unwrap();
                writeln!(buf, "#align(center, [_By keeping this budget, you should be able to recover from your overspending_ (#text([`{:.0}`], fill: {})) _by_\\ *{}* #h(1em) (in {:.0}{} days).])", overspent_total, color, recover_date.format("%B %-d, %Y"), recover_time_days, recover_date_drift_symbol).unwrap();
            }
            writeln!(buf, "]))").unwrap();

        } else if -*accumulated.last().unwrap() > budget.accumulated_general(today, today + TimeDelta::days(7)) {
            let mut days = 7;
            let spared = -*accumulated.last().unwrap();
            while spared > budget.accumulated_general(today, today + TimeDelta::days(days)) {
                days += 1;
            }
            days = days * 10 / 11;
            writeln!(buf, "#align(center, box(radius: 2em, stroke: 2pt + black, inset: 2em, [").unwrap();
            writeln!(buf, "#align(center, [You spared ] + text(fill: green, [`{:.0}`]) + [\\ Under your budget plan that's around {} days' worth.])", spared, days).unwrap();
            writeln!(buf, "]))").unwrap();
            writeln!(buf, "#v(3em)").unwrap();
            
        }

        {
            let accumulated_length = accumulated.len().min(365);
            let (data_str, fill_gradient, stroke_gradient, accum_points) = {
                let mut data_str_buf = Vec::new();
                if accumulated.len() > 365 {
                    accumulated = accumulated.split_off(accumulated.len() - 365);
                    assert!(accumulated.len() == 365);
                }
                for (i, x) in accumulated.iter().enumerate() {
                    write!(data_str_buf, "({},{:.2}),", i, x).unwrap();
                }

                let max = accumulated.to_owned().into_iter().reduce(Money::max).unwrap();
                let min = accumulated.to_owned().into_iter().reduce(Money::min).unwrap();
                
                let percentage = if max == min {
                    if max > dec!(0) {
                        dec!(-100.0)
                    } else {
                        dec!(100.0)
                    }
                } else {
                    max * dec!(100.0)/(max-min)
                };
                let epsilon = dec!(1e-1);

                if percentage <= dec!(0.0) {
                    (
                        format!("({})", String::from_utf8(data_str_buf).unwrap()),
                        format!("(green.transparentize(100%), 0%), (green.transparentize(66%), 100%)"),
                        format!("(green, 0%), (green, 100%)"),
                        accumulated.iter().enumerate().collect::<Vec<_>>(),
                    )
                } else if percentage >= dec!(100.0) {
                    (
                        format!("({})", String::from_utf8(data_str_buf).unwrap()),
                        format!("(red.transparentize(33%), 0%), (red.transparentize(100%), 100%)"),
                        format!("(red, 0%), (red, 100%)"),
                        accumulated.iter().enumerate().collect::<Vec<_>>(),
                    )
                } else {
                    (
                        format!("({})", String::from_utf8(data_str_buf).unwrap()),
                        format!("(red.transparentize(33%), 0%), (red.transparentize(100%), {}%), (green.transparentize(100%), {}%), (green.transparentize(66%), 100%)", percentage - epsilon, percentage + epsilon),
                        format!("(red, 0%), (red, {}%), (green, {}%), (green, 100%)", percentage - epsilon, percentage + epsilon),
                        accumulated.iter().enumerate().collect::<Vec<_>>(),
                    )
                }
            };

            const PREDICTION_LOOKAHEAD_DAYS: usize = 30;
            let (recovery_points_str, recovery_points) = {
                let mut points = Vec::with_capacity(recover_time_days.ceil().as_i128() as usize);
                let mut overspent = overspent_total;
                let mut idx = accumulated_length.max(1) - 1;
                let fraction = dec!(1.0) - fraction;
                while overspent > dec!(0.0) && points.len() < PREDICTION_LOOKAHEAD_DAYS {
                    points.push((idx, overspent));
                    let days_delta = idx as i64 - accumulated_length as i64 + 1;
                    overspent -= fraction * budget.general_at(today + TimeDelta::days(days_delta));
                    idx += 1;
                }
                if overspent <= dec!(0.0) {
                    points.push((idx, dec!(0.0).min(overspent_total)));
                }
                while points.len() < PREDICTION_LOOKAHEAD_DAYS {
                    points.push((idx, dec!(0.0).min(overspent_total)));
                    idx += 1;
                }

                let mut data_str_buf = Vec::new();
                for (i, x) in points.iter() {
                    write!(data_str_buf, "({},{:.2}),", i, x).unwrap();
                }
                (format!("({})", String::from_utf8(data_str_buf).unwrap()), points)
            };

            let (important_indices, important_dates) = {
                let mut first_days = vec![];
                let mut first_dates = vec![];
                let mut current = today - TimeDelta::days(accumulated_length as i64);
                let mut idx = 0;
                while current <= today + TimeDelta::days(PREDICTION_LOOKAHEAD_DAYS as i64) {
                    if current.day() == 1 {
                        first_days.push(idx);
                        first_dates.push(current);
                    }
                    current += TimeDelta::days(1);
                    idx += 1;
                }
                (first_days, first_dates)
            };

            let all_points =  accum_points.into_iter().map(|x| (x.0, *x.1)).chain(recovery_points.into_iter()).collect::<HashMap<_,_>>();

            const CANVAS_SIZE_X: usize = 15;
            writeln!(buf, "#v(1em)").unwrap();
            writeln!(buf, "#align(center,").unwrap();
            writeln!(buf, "cetz.canvas({{").unwrap();
            writeln!(buf, "import cetz.draw: *").unwrap();
            writeln!(buf, "import cetz-plot: *").unwrap();
            let mut max_y = all_points.iter().map(|x| *x.1).max().unwrap();
            let mut min_y = all_points.iter().map(|x| *x.1).min().unwrap();
            let delta = (max_y - min_y) * dec!(0.075);
            max_y += delta;
            min_y -= delta;
            let last_pt = vec![all_points.len()];
            for ((x, next_x), d) in important_indices.iter().zip(important_indices.iter().skip(1).chain(last_pt.iter())).zip(important_dates) {
                let mut avg_y = dec!(0.0);
                for i in *x..*next_x {
                    avg_y += all_points[&i];
                }
                avg_y /= Money::from(next_x - x).max(dec!(1));

                // let y = all_points.get(&x).unwrap_or(&dec!(0.0)).to_owned();
                let color = if avg_y > dec!(0.0) { "red" } else { "green" };
                // println!("{} {} {} {} {}", d.month(), x, next_x, avg_y, color);
                // writeln!(buf, "    content(({}, {}), [_{:02}_])", *x as f64 * CANVAS_SIZE_X as f64 / all_points.len() as f64, 0, d.month()).unwrap();
                writeln!(buf, "    content(({},{}), text([_{:02}_], 24pt, fill: {}.transparentize(90%)), anchor: \"south-west\")", *x as f64 * CANVAS_SIZE_X as f64 / all_points.len() as f64, 0, d.month(), color).unwrap();
            }
            writeln!(buf).unwrap();
            writeln!(buf, "plot.plot(").unwrap();
            writeln!(buf, "    size: ({}, 3),", CANVAS_SIZE_X).unwrap();
            writeln!(buf, "    axis-style: none,").unwrap();
            writeln!(buf, "    {{").unwrap();
            for x in important_indices {
                let y = all_points.get(&x).unwrap_or(&dec!(0.0)).to_owned();
                let color = if y > dec!(0.0) { "red" } else { "green" };
                writeln!(buf, "    plot.add((({}, {:.2}),({},{:.2})), style: (stroke: 0.75pt + gradient.linear(({}.transparentize(100%), 0%),({}.transparentize(90%), 25%),({}.transparentize(90%), 75%),({}.transparentize(100%), 100%), dir: direction.ttb), fill: none))", x, min_y, x, max_y, color, color, color, color).unwrap();
            }
            writeln!(buf, "    plot.add(").unwrap();
            writeln!(buf, "        {},", data_str).unwrap();
            writeln!(buf, "        fill: true,").unwrap();
            writeln!(buf, "        style: (stroke: gradient.linear({}, dir: direction.ttb), fill: gradient.linear({}, dir: direction.ttb)),", stroke_gradient, fill_gradient).unwrap();
            writeln!(buf, "    )").unwrap();
            writeln!(buf, "    plot.add(").unwrap();
            writeln!(buf, "        {},", recovery_points_str).unwrap();
            writeln!(buf, "        fill: true,").unwrap();
            writeln!(buf, "        style: (stroke: (dash: \"dashed\", paint: gradient.linear((black.transparentize(67%), 0%), (black.transparentize(85%), 100%), dir: direction.ltr)), fill: none),").unwrap();
            writeln!(buf, "    )").unwrap();
            for (m_avg, start, end) in monthly_averages(today, &accumulated) {
                let color = if m_avg > dec!(0) { "red" } else { "green" };
                writeln!(buf, "    plot.add(").unwrap();
                writeln!(buf, "        (({}, {:.2}), ({},{:.2})),", accumulated.len() - start-1, m_avg, accumulated.len()-end-1, m_avg).unwrap();
                writeln!(buf, "        fill: true,").unwrap();
                writeln!(buf, "        style: (stroke: (paint: gradient.linear(({}.transparentize(100%), 0%), ({}.transparentize(67%), 50%),({}.transparentize(100%), 100%), dir: direction.ltr)), fill: none),", color, color, color).unwrap();
                writeln!(buf, "    )").unwrap();
            }
            writeln!(buf, "    }}").unwrap();
            writeln!(buf, ")").unwrap();
            writeln!(buf, "}}))").unwrap();
        }
        
        writeln!(buf, "#v(1.5fr)").unwrap();
    }

    writeln!(buf, "").unwrap();


    writeln!(buf, "").unwrap();
    writeln!(buf, "= 5 Year Overview").unwrap();
    writeln!(buf, "").unwrap();
        let mut total = dec!(0.0);
        let mut total_days = 0;
        for (y, y_stats) in stats.yearly.iter().rev().zip(0..5).map(|x| x.0).rev() {
            let year_start = if stats.start.year() == *y {
                y_stats.start
            } else {
                NaiveDate::from_ymd_opt(*y,1, 1).unwrap()
            };
            let d_total_days = if stats.start.year() == *y {
                days_in_year(year_start) - (year_start - NaiveDate::from_ymd_opt(*y,1, 1).unwrap()).num_days()
            } else if stats.end.year() == *y && today.year() == *y {
                days_in_year(year_start) - ((NaiveDate::from_ymd_opt(*y+1,1, 1).unwrap() - today).num_days() - 1)
            } else {
                days_in_year(year_start)
            };
            total += y_stats.total;
            total_days += d_total_days;
        }

        let average: Money = total * dec!(365.0) / Decimal::from(total_days);
        let accumulated_total_days = budget.accumulated_general(today - TimeDelta::days(total_days-1), today);
        let average_budget: Money = accumulated_total_days*dec!(365.0)/Decimal::from(total_days);
        let overspending = total - accumulated_total_days;
        let percentage =  average*dec!(100.0)/average_budget;
        let color = percentage_to_color(percentage/dec!(100.0));
        write!(buf, "#align(center, [#text([`{:.0}`], fill: {}) in average per 365 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.0}` _(budget)_\\ ", percentage, average_budget).unwrap();
        if percentage < dec!(95.0) {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.0}`], fill: {})!])])", -overspending, color).unwrap();
        } else if percentage > dec!(100.0) {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.0}`], fill: {})!])])", overspending, color).unwrap();
        } else {
            writeln!(buf, "#text(8pt, [You are on budget])])").unwrap();
        }

        writeln!(buf).unwrap();
        writeln!(buf, "#v(1em)").unwrap();
        writeln!(buf).unwrap();

        writeln!(buf, "#align(center)[#cetz.canvas({{").unwrap();
        writeln!(buf, "import cetz.draw: *").unwrap();
        writeln!(buf, "import cetz-plot: *").unwrap();
        writeln!(buf, "chart.columnchart((").unwrap();
        for (y, y_stats) in stats.yearly.iter().rev().zip(0..5).map(|x| x.0).rev() {
            let year_start = if stats.start.year() == *y {
                y_stats.start
            } else {
                NaiveDate::from_ymd_opt(*y,1, 1).unwrap()
            };
            let days = if stats.start.year() == *y {
                days_in_year(year_start) - (year_start - NaiveDate::from_ymd_opt(*y,1, 1).unwrap()).num_days()
            } else if stats.end.year() == *y && today.year() == *y {
                days_in_year(year_start) - ((NaiveDate::from_ymd_opt(*y+1,1, 1).unwrap() - today).num_days() - 1)
            } else {
                days_in_year(year_start)
            };
            let allowed = budget.accumulated_general(year_start, year_start + TimeDelta::days(days));
            if y_stats.total > allowed {
                writeln!(buf, "([{}], ({}, {})),", y, allowed, y_stats.total - allowed).unwrap();
            } else if today.year() == *y {
                writeln!(buf, "([{}], ({}, 0, {})),", y, y_stats.total, allowed - y_stats.total).unwrap();
            } else {
                writeln!(buf, "([{}], {}),", y, y_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (16, 8), bar-style: cetz.palette.new(dash: (\"solid\", \"solid\", \"dashed\"), colors: (black.lighten(85%), red.lighten(50%), black.transparentize(100%))), x-label: [Year], y-label: [Amount spent])").unwrap();
        writeln!(buf, "}})]").unwrap();

    writeln!(buf, "").unwrap();
    writeln!(buf, "= 12 Month Overview").unwrap();
    writeln!(buf, "").unwrap();
        let mut total = dec!(0.0);
        let mut total_days = 0;
        for ((y, m), m_stats) in stats.monthly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            let month_start = if stats.start.month() == *m && stats.start.year() == *y {
                m_stats.start
            } else {
                NaiveDate::from_ymd_opt(*y,*m, 1).unwrap()
            };
            let d_total_days = if stats.start.year() == *y && stats.start.month() == *m {
                days_in_month(month_start) - (month_start - NaiveDate::from_ymd_opt(*y,*m, 1).unwrap()).num_days()
            } else if stats.end.year() == *y && stats.start.month() == *m && today.year() == *y && today.month() == *m {
                days_in_month(month_start) - ((next_month(month_start) - today).num_days() - 1)
            } else {
                days_in_month(month_start)
            };
            total += m_stats.total;
            total_days += d_total_days;
        }

        let average = total * dec!(30.0) / Decimal::from(total_days);
        let accumulated_total_days = budget.accumulated_general(today - TimeDelta::days(total_days-1), today);
        let average_budget =accumulated_total_days*dec!(30.0)/Decimal::from(total_days);
        let percentage =  average*dec!(100.0)/average_budget;
        let color = percentage_to_color(percentage/dec!(100.0));
        let overspent = total - accumulated_total_days;
        write!(buf, "#align(center, [#text([`{:.0}`], fill: {}) in average per 30 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.0}` _(budget)_\\ ", percentage, average_budget).unwrap();
        if percentage < dec!(95.0) {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.0}`], fill: {})!])])", -overspent, color).unwrap();
        } else if percentage > dec!(100.0) {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.0}`], fill: {})!])])", overspent, color).unwrap();
        } else {
            writeln!(buf, "#text(8pt, [You are on budget])])").unwrap();
        }

        writeln!(buf).unwrap();
        writeln!(buf, "#v(1em)").unwrap();
        writeln!(buf).unwrap();

        writeln!(buf, "#align(center)[#cetz.canvas({{").unwrap();
        writeln!(buf, "import cetz.draw: *").unwrap();
        writeln!(buf, "import cetz-plot: *").unwrap();
        writeln!(buf, "chart.columnchart((").unwrap();
        for ((y, m), m_stats) in stats.monthly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            let month_start = if stats.start.month() == *m && stats.start.year() == *y {
                m_stats.start
            } else {
                NaiveDate::from_ymd_opt(*y,*m, 1).unwrap()
            };
            let n_days = if stats.start.year() == *y && stats.start.month() == *m {
                days_in_month(month_start) - (month_start - NaiveDate::from_ymd_opt(*y,*m, 1).unwrap()).num_days()
            } else if stats.end.year() == *y && stats.end.month() == *m && today.year() == *y && today.month() == *m {
                days_in_month(month_start) - ((next_month(month_start) - today).num_days() - 1)
            } else {
                days_in_month(month_start)
            };
            let allowed = budget.accumulated_general(month_start, month_start + TimeDelta::days(n_days));
            if m_stats.total > allowed {
                writeln!(buf, "([{:02}/{}], ({}, {})),", m, y%100, allowed, m_stats.total - allowed).unwrap();
            } else if today.month() == *m  && today.year() == *y {
                writeln!(buf, "([{:02}/{}], ({}, 0, {})),", m, y%100, m_stats.total, allowed - m_stats.total).unwrap();
            } else {
                writeln!(buf, "([{:02}/{}], {}),", m, y%100, m_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (14, 8), bar-style: cetz.palette.new(dash: (\"solid\", \"solid\", \"dashed\"), colors: (black.lighten(85%), red.lighten(50%), black.transparentize(100%))), x-label: [Month], y-label: [Amount spent])").unwrap();
        writeln!(buf, "}})]").unwrap();

    writeln!(buf, "").unwrap();
    writeln!(buf, "= 12 Weeks Overview").unwrap();
    writeln!(buf, "").unwrap();
        let mut total = dec!(0.0);
        let total_days = 7*11 + today.weekday().num_days_from_sunday() as i64;
        for (_, m_stats) in stats.weekly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            total += m_stats.total;
        }

        let average = total * dec!(7.0) / Decimal::from(total_days);
        let accumulated_total_days = budget.accumulated_general(today - TimeDelta::days(total_days-1), today);
        let average_budget = accumulated_total_days*dec!(7.0)/Decimal::from(total_days);
        let percentage =  average*dec!(100.0)/average_budget;
        let overspent = total - accumulated_total_days;
        let color = percentage_to_color(percentage/dec!(100.0));
        write!(buf, "#align(center, [#text([`{:.0}`], fill: {}) in average per 7 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.0}` _(budget)_\\ ", percentage, average_budget).unwrap();
        if percentage < dec!(95.0) {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.0}`], fill: {})!])])", -overspent, color).unwrap();
        } else if percentage > dec!(100.0) {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.0}`], fill: {})!])])", overspent, color).unwrap();
        } else {
            writeln!(buf, "#text(8pt, [You are on budget])])").unwrap();
        }

        writeln!(buf).unwrap();
        writeln!(buf, "#v(1em)").unwrap();
        writeln!(buf).unwrap();

        writeln!(buf, "#align(center)[#cetz.canvas({{").unwrap();
        writeln!(buf, "import cetz.draw: *").unwrap();
        writeln!(buf, "import cetz-plot: *").unwrap();
        writeln!(buf, "chart.columnchart((").unwrap();
        for (week, w_stats) in stats.weekly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            let week_start = NaiveDate::from_isoywd_opt(week.year(), week.week(), Weekday::Mon).unwrap();
            let week_end = if today.iso_week() == *week {
                week_start + TimeDelta::days(today.signed_duration_since(week_start).num_days())
            } else {
                week_start + TimeDelta::days(7 - 1)
            };
            let allowed = budget.accumulated_general(week_start, week_end);
            let label = if (week_start - TimeDelta::days(7)).month() != week_start.month() {
                format!("#underline[{:02}/{:02}]", week_start.day(), week_start.month())
            } else {
                format!("{:02}/{:02}", week_start.day(), week_start.month())
            };
            if w_stats.total > allowed {
                writeln!(buf, "(text(10pt, [{}]), ({}, {})),", label, allowed, w_stats.total - allowed).unwrap();
            } else if today.iso_week() == *week {
                writeln!(buf, "(text(10pt,[{}]), ({}, 0, {})),", label, w_stats.total, allowed - w_stats.total).unwrap();
            } else {
                writeln!(buf, "(text(10pt,[{}]), {}),", label, w_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (12, 8), bar-style: cetz.palette.new(dash: (\"solid\", \"solid\", \"dashed\"), colors: (black.lighten(85%), red.lighten(50%), black.transparentize(100%))), x-label: [Week], y-label: [Amount spent])").unwrap();
        writeln!(buf, "}})]").unwrap();

    writeln!(buf, "").unwrap();
    writeln!(buf, "= Data").unwrap();
        writeln!(buf, "").unwrap();
        let mut ns =stats.last_n_days.keys().collect::<Vec<_>>();
        ns.sort();
        for n_days in ns {
            write_typ_table(&mut buf, stats, budget, *n_days);
        }

    writeln!(buf, "= Overview of Weekdays").unwrap();
        writeln!(buf, "#columns(2)[").unwrap();
        for n_days in LAST_N_DAYS {
            if n_days < 14 {
                continue;
            }
            writeln!(buf, "#align(center, text([*Last {} days*], 14pt))", n_days).unwrap();
            writeln!(buf, "#align(center)[#cetz.canvas({{").unwrap();
            writeln!(buf, "import cetz.draw: *").unwrap();
            writeln!(buf, "import cetz-plot: *").unwrap();
            writeln!(buf, "chart.columnchart((").unwrap();
            let last_days_stats = &stats.last_n_days[&n_days];
            for (wd, amount) in last_days_stats.by_day_of_week.iter() {
                    writeln!(buf, "(text(8pt,[{}]), {:.2}),", wd, amount / last_days_stats.total * dec!(100.0)).unwrap();
            }
            writeln!(buf, "), mode: \"stacked\", size: (8, 4), bar-style: cetz.palette.new(dash: (\"solid\", \"solid\", \"dashed\"), colors: (black.lighten(85%), red.lighten(50%), black.transparentize(100%))), x-label: [Weekday], y-label: [Amount spent (%)])").unwrap();
            writeln!(buf, "}})]").unwrap();
        }
        writeln!(buf, "]").unwrap();

    writeln!(buf, "").unwrap();
    let mut f = std::fs::File::create(file_path).unwrap();
    f.write(buf.as_slice()).unwrap();
}

fn parse_raw_xml(file_path: &PathBuf) -> Vec<DBEntry> {
    use DBEntry::*;
    let content = fs::read_to_string(file_path).unwrap_or_default();
    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    let mut db_entries = vec![];
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match e.name().as_ref() {
                    b"budget" => {
                        let mut category = None;
                        let mut amount = None;
                        let mut duration = None;
                        let mut date = None;

                        for attr in e.attributes() {
                            let attr = attr.unwrap();
                            let key = String::from_utf8_lossy(&attr.key.0).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();

                            match key.as_str() {
                                "category" => category = Some(value),
                                "amount" => amount = Some(value),
                                "duration" => duration = Some(value),
                                "date" => date = Some(value),
                                _ => {}
                            }
                        }

                        if let (Some(amount), Some(duration), Some(date)) = (amount, duration, date) {
                            db_entries.push(Budget {
                                category,
                                amount,
                                duration,
                                date,
                            });
                        } else {
                            eprintln!("[WARNING]: Incomplete tag: `{}`.", String::from_utf8(e.to_vec()).unwrap());
                        }
                    }
                    b"transaction" => {
                        let mut amount = None;
                        let mut category = None;
                        let mut date = None;
                        let mut payment_method = None;
                        let mut note = None;

                        for attr in e.attributes() {
                            let attr = attr.unwrap();
                            let key = String::from_utf8_lossy(&attr.key.0).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();

                            match key.as_str() {
                                "amount" => amount = Some(value),
                                "category" => category = Some(value),
                                "date" => date = Some(value),
                                "payment-method" => payment_method = Some(value),
                                "note" => note = Some(value),
                                _ => {}
                            }
                        }

                        if let (Some(amount), Some(category), Some(date), Some(payment_method)) =
                            (amount, category, date, payment_method) {
                            db_entries.push(Transaction{
                                amount,
                                category,
                                date,
                                payment_method,
                                note: note.unwrap_or_default(),
                            });
                        } else {
                            eprintln!("[WARNING]: Incomplete tag: {}", String::from_utf8(e.to_vec()).unwrap());
                        }
                    },
                    b"extra" => {
                        let mut amount = None;
                        let mut date = None;
                        let mut payment_method = None;
                        let mut note = None;

                        for attr in e.attributes() {
                            let attr = attr.unwrap();
                            let key = String::from_utf8_lossy(&attr.key.0).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();

                            match key.as_str() {
                                "amount" => amount = Some(value),
                                "date" => date = Some(value),
                                "payment-method" => payment_method = Some(value),
                                "note" => note = Some(value),
                                _ => {}
                            }
                        }

                        if let (Some(amount), Some(date), Some(payment_method)) =
                            (amount, date, payment_method) {
                            db_entries.push(Extra{
                                amount,
                                date,
                                payment_method,
                                note: note.unwrap_or_default(),
                            });
                        } else {
                            eprintln!("[WARNING]: Incomplete tag: {}", String::from_utf8(e.to_vec()).unwrap());
                        }
                    },
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!("[ERROR]: XML parsing error `{}`", e);
                exit(1);
            }
            _ => {}
        }
        buf.clear();
    }

    return db_entries;
}

fn write_xml_file(file_path: &PathBuf, db_entries: &mut Vec<DBEntry>) -> std::io::Result<()> {
    use DBEntry::*;
    let get_date = |e: &DBEntry| -> NaiveDate {
        let s = match e {
            Budget { date, .. } | Transaction { date, .. } | Extra { date, .. } => date,
        };
        NaiveDate::parse_from_str(s, "%d/%m/%Y").unwrap()
    };

    let type_priority = |e: &DBEntry| -> u8 {
        match e {
            Transaction { .. } => 0,
            Extra { .. }      => 1,
            Budget { .. }      => 2,
        }
    };

    db_entries.sort_by(|a, b| {
        match get_date(b).cmp(&get_date(a)) {
            Ordering::Equal => {}
            ord => return ord,
        }

        match type_priority(a).cmp(&type_priority(b)) {
            Ordering::Equal => {}
            ord => return ord,
        }

        match (a, b) {
            (Transaction { amount: a_amt, .. }, Transaction { amount: b_amt, .. })
            | (Extra     { amount: a_amt, .. }, Extra      { amount: b_amt, .. }) => b_amt
                .parse::<Money>()
                .unwrap()
                .partial_cmp(&a_amt.parse::<Money>().unwrap())
                .unwrap(),

            (
                Budget { category: a_cat, amount: a_amt, duration: a_dur, .. },
                Budget { category: b_cat, amount: b_amt, duration: b_dur, .. },
            ) => {
                match (a_cat.is_some(), b_cat.is_some()) {
                    (false, true) => return Ordering::Less,
                    (true, false) => return Ordering::Greater,
                    _ => {}
                }
                let a_per_day = a_amt.parse::<Money>().unwrap()
                    / a_dur.parse::<Money>().unwrap();
                let b_per_day = b_amt.parse::<Money>().unwrap()
                    / b_dur.parse::<Money>().unwrap();
                b_per_day.partial_cmp(&a_per_day).unwrap()
            }

            _ => Ordering::Equal,
        }
    });

    let mut content = String::new();

    for e in db_entries {
        match e {
            Budget{amount, category, duration, date} => {
                if let Some(category) = category {
                    content.push_str(&format!(
                        "<budget category=\"{}\" amount=\"{}\" duration=\"{}\" date=\"{}\"/>\n",
                        category, amount, duration, date
                    ));
                } else {
                    content.push_str(&format!(
                        "<budget amount=\"{}\" duration=\"{}\" date=\"{}\"/>\n",
                        amount, duration, date
                    ));
                }
            },
            Transaction{amount, category, date, payment_method, note} => {
                content.push_str(&format!(
                    "<transaction amount=\"{}\" category=\"{}\" date=\"{}\" payment-method=\"{}\" note=\"{}\"/>\n",
                    amount, category, date, payment_method, note
                ));
            },
            Extra{amount, date, payment_method, note} => {
                content.push_str(&format!(
                    "<extra amount=\"{}\" date=\"{}\" payment-method=\"{}\" note=\"{}\"/>\n",
                    amount, date, payment_method, note
                ));
            }
        }

    }

    fs::write(file_path, content)
}

fn print_usage() {
    println!("USAGE: {} [add] <path/to/file.xml>", env::args().next().unwrap());
}

fn get_options() -> (Option<PathBuf>, bool) {
    let args = env::args().skip(1);
    let mut add = false;

    let mut path = None;
    for arg in args {
        if arg == "add" {
            add = true;
        } else {
            let cur_path = PathBuf::from(arg);
            path = Some(cur_path);
            break;
        }
    }

    return (path, add);
}

fn prompt_with_default(prompt: &str, default: &str) -> String {
    print!(" {} [{}] > ", prompt, default);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_string();

    if input.is_empty() {
        default.to_string()
    } else {
        input
    }
}

fn prompt_date_with_default(default: &str) -> String {
    let default_date = NaiveDate::parse_from_str(&default, "%d/%m/%Y").unwrap();
    print!(" Date [{}] (or 'today') > ", default);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_string();

    if input.is_empty() {
        default.to_string()
    } else if input.to_lowercase() == "today" {
        Local::now().date_naive().format("%d/%m/%Y").to_string()
    } else {
        if NaiveDate::parse_from_str(&input, "%d/%m/%Y").is_ok() {
            input
        } else if input.split('/').all(|x| x.parse::<u32>().is_ok()) {
            let numbers = input.split('/').map(|x| x.parse::<u32>().unwrap()).collect::<Vec<_>>();
            let corrected_input = match numbers.len() {
                1 => format!("{:02}/{:02}/{}", input.parse::<u32>().unwrap(), default_date.month(), default_date.year()),
                2 => format!("{:02}/{:02}/{}", numbers[0], numbers[1], default_date.year()),
                _ =>  {
                    eprintln!("[ERROR] Invalid date format. Please use dd/mm/yyyy.");
                    prompt_date_with_default(default)
                },
            };
            if NaiveDate::parse_from_str(&corrected_input, "%d/%m/%Y").is_ok() {
                corrected_input
            } else {
                eprintln!("[ERROR] Invalid date format. Please use dd/mm/yyyy.");
                prompt_date_with_default(default)
            }
        } else {
            eprintln!("[ERROR] Invalid date format. Please use dd/mm/yyyy.");
            prompt_date_with_default(default)
        }
    }
}

fn add_transactions_interactive(file_path: &PathBuf) -> std::io::Result<()> {
    fs::copy(file_path, format!("{}.bak", file_path.display())).unwrap();
    let mut entries = parse_raw_xml(file_path);
    
    let transactions = entries.clone().into_iter().filter(
        |x| match x {DBEntry::Transaction{..} => true, _ => false}
    ).map(RawTransaction::from).collect::<Vec<_>>();

    let (mut default_date, mut default_category, mut default_payment_method) = if !transactions.is_empty() {
        let mut sorted_transactions = transactions.clone();
        sorted_transactions.sort_by(|a, b| {
            let date_a = NaiveDate::parse_from_str(&a.date, "%d/%m/%Y")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
            let date_b = NaiveDate::parse_from_str(&b.date, "%d/%m/%Y")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
            date_b.cmp(&date_a)
        });

        let last = &sorted_transactions[0];
        (
            last.date.clone(),
            last.category.clone(),
            last.payment_method.clone()
        )
    } else {
        (
            Local::now().date_naive().format("%d/%m/%Y").to_string(),
            String::new(),
            String::new()
        )
    };
    
    let last_date = transactions.iter().map(|x| NaiveDate::parse_from_str(&x.date, "%d/%m/%Y").unwrap()).max().unwrap();
    let mut last_transactions =  transactions.iter().filter(|x| NaiveDate::parse_from_str(&x.date, "%d/%m/%Y").unwrap() == last_date).map(|x| x.clone()).collect::<Vec<_>>();

    let mut added_transaction_idx = 0;
    loop {
        added_transaction_idx += 1;
        if added_transaction_idx != 1 {
            println!("{}\n", tclear());
        }
        let bold_title =  tbold(&format!("=== Add Transaction (#{}) ===", added_transaction_idx));
        println!("{}", bold_title);
        if !last_transactions.is_empty() {
            if added_transaction_idx != 1 {
                println!("Added transactions:");
                for (i, t) in   last_transactions.iter().enumerate() {
                    println!(" {}. {}", i+1, t);
                }
            } else {
                println!("Last transactions:");
                for t in   last_transactions.iter() {
                    println!(" - {}", t);
                }
            }
        }
        if added_transaction_idx == 1 {
            last_transactions.clear();
        }
        println!();
        
        
        println!("Transaction data:");
        let date = prompt_date_with_default(&default_date);
        let amount = loop {
            print!(" Amount > ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let amount_str = input.trim();

            if validate_amount(amount_str) {
                break amount_str.to_string();
            } else {
                println!("Please enter a valid amount (e.g., 15.50)");
            }
        };

        let category = if default_category.is_empty() {
            print!(" Category > ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            input.trim().to_string()
        } else {
            prompt_with_default("Category", &default_category)
        };

        let payment_method = if default_payment_method.is_empty() {
            print!(" Payment Method > ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            input.trim().to_string()
        } else {
            prompt_with_default("Payment Method", &default_payment_method)
        };

        print!(" Note (optional) > ");
        io::stdout().flush().unwrap();
        let mut note = String::new();
        io::stdin().read_line(&mut note).unwrap();
        let note = note.trim().to_string();

        default_category = category.clone();
        default_date = date.clone();
        default_payment_method = payment_method.clone();

        let new_transaction = RawTransaction {
            amount,
            category: category.clone(),
            date: date.clone(),
            payment_method: payment_method.clone(),
            note,
        };

        last_transactions.push(new_transaction.clone());
        entries.push(DBEntry::from(new_transaction));
        write_xml_file(file_path, &mut entries)?;
        println!("[INFO] Transaction added.");

        print!("Add another transaction? (y/n) > ");
        io::stdout().flush().unwrap();
        let mut response = String::new();
        io::stdin().read_line(&mut response).unwrap();

        let continue_condition = response.trim().to_lowercase().starts_with("y") || response.trim().is_empty();
        if !continue_condition {
            break;
        }
    }

    Ok(())
}

fn main() {
    let (path, add) = get_options();

    if path.is_none() {
        eprintln!("[ERROR] No file provided.");
        print_usage();
        exit(1);
    }

    assert!(path.is_some(), "Rust has a problem here.");
    let path = path.unwrap();
    match path.try_exists() {
        Ok(true) => {},
        _ => {
            eprintln!("[ERROR] The provided filepath `{}` does not exist.", path.display());
            exit(1);
        }
    }


    if add {
        match add_transactions_interactive(&path) {
            Ok(_) => println!("[INFO] Transaction addition completed."),
            Err(e) => {
                eprintln!("[ERROR] Failed to add transactions: `{}`.", e);
                exit(1);
            },
        }
    }

    let (transactions, budget) = parse_file(&path);

    println!("[INFO] Read file `{}`.", path.display());

    if transactions.is_empty() {
        println!("[INFO] Provided file `{}` has no transactions. Exiting...", path.display());
        return;
    }

    let stats = get_stats(&transactions);

    let mut out_path = path.clone();
    out_path.set_extension("typ");
    write_typ_report(&out_path, &stats, &budget, &path);
    println!("[INFO] Analyzed spending and budget data.");

    let compile_output = Command::new("typst").arg("compile").arg(&out_path).output();
    match compile_output {
        Err(_) => {
            eprintln!("[ERROR] Unable to compile report.");
            println!("[INFO] Typst source code saved in  `{}`.", out_path.display());
            exit(1);
        },
        Ok(x) => {
            if !x.status.success() {
                eprintln!("[ERROR] Unable to compile report.");
                println!("[INFO] Typst source code saved in  `{}`.", out_path.display());
                exit(1);
            } else {
                let _ = fs::remove_file(&out_path);
                let mut out_pdf_path = out_path;
                out_pdf_path.set_extension("pdf");
                println!("[INFO] Detailed report saved in `{}`.", out_pdf_path.display());
            }
        }
    }
}
