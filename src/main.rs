use core::fmt;
use std::{
    cmp::Ordering, collections::{BTreeMap, HashMap}, env, fmt::Debug, fs, io::{self, Write}, path::PathBuf
};

use chrono::{Datelike, IsoWeek, Local, NaiveDate, TimeDelta, Weekday};
use quick_xml::{Reader, events::Event};
use rust_decimal::Decimal;
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
struct RawBudget {
    category: Option<String>,
    amount: String,
    duration: String,
    date: String,
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

#[derive(Debug, Default, Clone)]
struct RateSchedule {
    changes: BTreeMap<NaiveDate, Money>,
}

impl RateSchedule {
    fn set(&mut self, date: NaiveDate, rate: Money) {
        self.changes.insert(date, rate);
    }

    #[allow(unused)]
    fn current_rate(&self) -> Money {
        let today = Local::now().date_naive();
        self.rate_at(today).unwrap_or(dec!(0.0))
    }

    fn rate_at(&self, date: NaiveDate) -> Option<Money> {
        let mut last = None;

        for (d, rate) in &self.changes {
            if *d > date {
                break;
            }
            last = Some(*rate);
        }

        last
    }

    fn accumulated(&self, start: NaiveDate, end: NaiveDate) -> Money {
        let mut total = Money::ZERO;
        let mut cursor = start + TimeDelta::days(1);

        while cursor <= end {
            total += self.rate_at(cursor).unwrap_or(Money::ZERO);
            cursor += TimeDelta::days(1);
        }

        total
    }
}

#[derive(Debug, Default, Clone)]
struct BudgetTimeline {
    general: RateSchedule,
    per_category: HashMap<Category, RateSchedule>,
}

impl BudgetTimeline {
    fn current_general(&self) -> Money {
        let today = Local::now().date_naive();
        self.general_budget_at(today)
    }

    fn general_budget_at(&self, date: NaiveDate) -> Money {
        let base = self.general.rate_at(date).unwrap_or(Money::ZERO);
        let category_sum = self.category_sum_at(date);
        base.max(category_sum)
    }

    #[allow(unused)]
    fn category_budget_at(&self, category: &str, date: NaiveDate) -> Option<Money> {
        self.per_category.get(category).and_then(|s| s.rate_at(date))
    }

    fn accumulated_days_to(&self, n_days: i64, start: NaiveDate) -> Money {
        if n_days < 0 {
            return self.accumulated(start, start + TimeDelta::days(-n_days));
        } else {
            return self.accumulated(start - TimeDelta::days(n_days), start);
        }
    }

    fn accumulated_days(&self, n_days: i64) -> Money {
        let today = Local::now().date_naive();
        return self.accumulated_days_to(n_days, today);
    }

    fn accumulated(&self, start: NaiveDate, end: NaiveDate) -> Money {
        let mut total = Money::ZERO;
        let mut cursor = start + TimeDelta::days(1);

        while cursor <= end {
            total += self.general_budget_at(cursor);
            cursor += TimeDelta::days(1);
        }

        total
    }

    fn category_accumulated(&self, category: &str, start: NaiveDate, end: NaiveDate) -> Option<Money> {
        self.per_category.get(category).map(|s| s.accumulated(start, end))
    }

    fn category_sum_at(&self, date: NaiveDate) -> Money {
        self.per_category
            .values()
            .map(|sched| sched.rate_at(date).unwrap_or(Money::ZERO))
            .sum()
    }

    fn add_general(&mut self, date: NaiveDate, amount: Money, duration: Money) {
        self.general.set(date, (amount / duration).round_dp(2));
    }

    fn add_category(&mut self, category: Category, date: NaiveDate, amount: Money, duration: Money) {
        self.per_category
            .entry(category)
            .or_default()
            .set(date, (amount / duration).round_dp(2));
    }
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
    average_transaction: Money,
    transaction_count: u64,
}

impl TempStats {
    pub fn update(&mut self, e: &Transaction) {
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

    pub fn calc_averages(&mut self, days: u64) {
        let days: Money = days.into();
        self.per_day = self.total / days;
        if self.transaction_count != 0 {
            self.average_transaction = self.total / Decimal::from(self.transaction_count);
        } else {
            assert!(self.total == dec!(0));
            self.average_transaction = self.total;
        }
    }

    pub fn into_stats(self) -> Stats {
        let mut by_category = self.by_category.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_category.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_payment_method = self.by_payment_method.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_payment_method.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_note = self.by_note.into_iter().map(|(k,v)| (k, v)).collect::<Vec<_>>();
        by_note.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        Stats {
            start: self.start,
            end: self.end,
            per_day: self.per_day,
            total: self.total,
            by_category,
            by_payment_method,
            by_note,
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
    pub fn into_stats_collection(self) -> StatsCollection {
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

    let mut result = Vec::new();
    let mut accumulated = Money::ZERO;
    let mut current = first_date;

    while current <= last_date {
        let daily_spending: Money = transactions
            .iter()
            .filter(|t| t.date == current)
            .map(|t| t.value)
            .sum();
        let daily_budget = budget.general_budget_at(current);
        let overspending = daily_spending - daily_budget;
        accumulated += overspending;
        result.push(accumulated);
        current = current + TimeDelta::days(1);
    }

    result
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
                    if let Some(category) = pot_category {
                        budget.add_category(category.to_owned(), NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap(), attributes.get("amount").unwrap().parse::<Money>().unwrap(), attributes.get("duration").unwrap().parse::<Money>().unwrap());
                    } else {
                        budget.add_general(NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap(), attributes.get("amount").unwrap().parse::<Money>().unwrap(), attributes.get("duration").unwrap().parse::<Money>().unwrap());
                    }
                },
                "transaction" => {
                    let attributes =  e.attributes().map(|x| {
                        let x = x.unwrap();
                        (String::from_utf8(x.key.as_ref().to_vec()).unwrap(), String::from_utf8(x.value.as_ref().to_vec()).unwrap())
                    }).collect::<HashMap<_,_>>();

                    assert!(attributes.get("amount").unwrap().chars().skip_while(|c| *c != '.').take_while(|c| c.is_numeric()).collect::<Vec<_>>().len() <= 2);
                    transactions.push(Transaction {
                        category: attributes.get("category").unwrap_or(&String::default()).trim().to_owned(),
                        date: NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap(),
                        value: attributes.get("amount").unwrap().parse::<Money>().unwrap(),
                        note: if attributes.contains_key("note") { attributes.get("note").unwrap().to_owned() } else { String::default() },
                        payment_method: attributes.get("payment-method").unwrap().trim().to_owned(),
                    });
                },
                x => {
                    println!("[ERROR]: Unknown tag: `{}`", x);
                },
            },
            Ok(_) => {},
            Err(e) => println!("[ERROR]: XML parsing error `{}`", e),
        }
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

fn write_typ_table(buf: &mut Vec<u8>, stats: &StatsCollection, budget: &BudgetTimeline, n_days: u64) {
    let today = Local::now().date_naive();
    let stats = stats.last_n_days.get(&n_days).unwrap();
    writeln!(buf, "== Last {} days", n_days).unwrap();
        writeln!(buf, "").unwrap();
        writeln!(buf, "#align(center, table(columns: 4, align: left, stroke: 0pt, column-gutter: 5pt, table.hline(stroke: 1pt), [*Category*], [*Amount*], [*% of Budget*], [*Allowed spending*],").unwrap();
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        for (category, amount) in stats.by_category.iter() {
            let allowed_amount = if let Some(allowed_amount) = budget.category_accumulated(category, today - TimeDelta::days(n_days as i64), today) {
                if budget.accumulated(today - TimeDelta::days(n_days as i64), today) > stats.total {
                    let allowed = allowed_amount - *amount;
                    (format!("{:.0}", allowed), format!("{:.0}%", (amount*dec!(100.0))/allowed_amount), if allowed >= dec!(0.0)  {
                        if allowed / allowed_amount >= dec!(0.25) {
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
                        if allowed / allowed_amount >= dec!(0.25) {
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
        let allowed_amount = if budget.current_general() > dec!(0.0) {
                let total_allowed = budget.accumulated(today - TimeDelta::days(n_days as i64), today);
                let allowed = total_allowed - stats.total;
                (format!("{:.0}", allowed), format!("{:.0}%", (stats.total*dec!(100.0))/total_allowed), if allowed >= dec!(0.0)  {
                    if allowed / total_allowed >= dec!(0.25) {
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
            writeln!(buf, "#v(2em)").unwrap();
            writeln!(buf, "").unwrap();
            writeln!(buf, "=== Biggest expenses (last {} days)", n_days).unwrap();
            writeln!(buf, "#align(center, table(columns: 3, stroke: 0pt, align: (right, left, right), ").unwrap();
            for ((note, amount),i) in stats.by_note.iter().filter(|x| x.1 > dec!(50.0)).zip(0..20) {
                writeln!(buf, "[{}.], [_\"{}\"_], [`{:.2}`], ", i+1, note, amount).unwrap();
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
    let mut budget_categories = budget.per_category.iter().map(|(c,b)| (c, b.accumulated(today, today + TimeDelta::days(30)))).collect::<Vec<_>>();
    budget_categories.sort_by_key(|(_,b)| -b);
    writeln!(buf, "#v(2em)").unwrap();
    writeln!(buf, "#columns(2, [").unwrap();
    writeln!(buf, "#align(center, text([*Monthly Budget*], 18pt)) ").unwrap();
    writeln!(buf, "#align(center, table(columns: 3, stroke: 0pt, align: (left, right, right), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Category*], align(left, [*Allowed monthly amount*]), align(left, [*% of Total*]), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    let mut total_allocated = dec!(0.0);
    for (category, monthly_budget) in budget_categories {
        writeln!(buf, "[{}], [`{:.0}`], [`{:.0}%`],", category, monthly_budget, (monthly_budget / budget.accumulated_days(-30))*dec!(100.0)).unwrap();
        total_allocated += monthly_budget;
        writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
    }
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[_Allocated total_], [], [`{:.0}%`],", total_allocated * dec!(100.0) / budget.accumulated_days(-30)).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Total*], [`{:.0}`], ", budget.accumulated_days(-30)).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "))").unwrap();
    writeln!(buf, "").unwrap();
    writeln!(buf, "#colbreak()").unwrap();
    writeln!(buf, "").unwrap();

    writeln!(buf, "#align(center, text([*Per Period*], 18pt)) ").unwrap();
    writeln!(buf, "#align(center, table(columns: 2, stroke: 0pt, align: (left, right), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Period*], align(left, [*Allowed amount*]), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "    [_Per month_], align(right, [`{:.0}`]),", budget.current_general() * dec!(30.0)).unwrap();
    writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
    writeln!(buf, "    [_Per week_],  align(right, [`{:.0}`]),", budget.current_general() * dec!(7.0)).unwrap();
    writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
    writeln!(buf, "    [_Per day_],   align(right, [`{:.0}`]),", budget.current_general()).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "))").unwrap();
    writeln!(buf, "])").unwrap();
    writeln!(buf, "#v(2fr)").unwrap();

    writeln!(buf, "").unwrap();
    const BUDGET_RECOVERY_PLAN_MIN_BUDGET_FRACTION: Money = dec!(0.55);
    {
        let mut accumulated = accumulated_overspending(&stats.transactions, budget);
        let current_budget = budget.current_general();
        let mut allowed_next_month = current_budget * dec!(30.0) + budget.accumulated(today - TimeDelta::days(30), today) - stats.last_n_days.get(&30).unwrap().total;
        let color = if allowed_next_month < current_budget * dec!(30.0) * dec!(0.80) { "red" } else if allowed_next_month < current_budget * dec!(30.0) * dec!(0.92) { "orange" } else { "black" };
        if allowed_next_month < current_budget * dec!(30.0) * dec!(0.75) {
            allowed_next_month = allowed_next_month.max(current_budget * dec!(30.0) * BUDGET_RECOVERY_PLAN_MIN_BUDGET_FRACTION * (dec!(1.0)/dec!(0.95)));
        }
        if allowed_next_month < budget.current_general() * dec!(30.0) || *accumulated.last().unwrap() > dec!(0.0) {
            writeln!(buf, "#pagebreak()").unwrap();
            writeln!(buf, "#v(3em)").unwrap();
            writeln!(buf, "#align(center, box(radius: 2em, stroke: 2pt + {}, inset: 2em, [", color).unwrap();
            let year_fraction = (dec!(1.0) - dec!(1.5) * (stats.last_n_days.get(&365).unwrap().total - current_budget * dec!(365.0))/(current_budget * dec!(365.0))).max(BUDGET_RECOVERY_PLAN_MIN_BUDGET_FRACTION / dec!(0.95)) * dec!(0.95);
            let month_fraction = allowed_next_month / (current_budget * dec!(30.0)) * dec!(0.95);
            let fraction = year_fraction.min(month_fraction).min(dec!(0.8));
            writeln!(buf, "#align(center,text(fill: {color}, [You have overspent in the last period.]) + [\\ For the next month, we suggest the following budget.])").unwrap();
            writeln!(buf, "#v(0.5em)").unwrap();
            writeln!(buf, "#align(center, table(columns: 2, stroke: 0pt, align: (left, right, right), ").unwrap();
            writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
            writeln!(buf, "    [*Period* #h(2em)], [*Allowed amount* (`{:.0}%` _of user budget_)],", fraction*dec!(100.0)).unwrap();
            writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
            writeln!(buf, "    [_Per month_], align(right, [`{:.0}`]),", fraction * current_budget * dec!(30.0)).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
            writeln!(buf, "    [_Per week_],  align(right, [`{:.0}`]),", fraction * current_budget * dec!(7.0)).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
            writeln!(buf, "    [_Per day_],   align(right, [`{:.0}`]),", fraction * current_budget).unwrap();
            writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
            writeln!(buf, "))").unwrap();
            let overspent_total = accumulated.last().unwrap();
            let recover_time_days = (overspent_total / ((dec!(1.0) - fraction) * current_budget) * dec!(1.1)).ceil();
            let recover_date = today + TimeDelta::days(recover_time_days.ceil().trunc().as_i128() as i64);
            writeln!(buf, "").unwrap();
            writeln!(buf, "#v(1em)").unwrap();
            writeln!(buf, "#align(center, [_By keeping this budget, you should be able to recover from your overspending_ (#text([`{:.0}`], fill: {})) _by_\\ *{}* #h(1em) (in {:.0} days).])", overspent_total, color, recover_date.format("%B %-d, %Y"), recover_time_days).unwrap();
            writeln!(buf, "]))").unwrap();
            

            
            let (data_str, fill_gradient, stroke_gradient) = {
                let mut data_str_buf = Vec::new();
                if accumulated.len() > 365 {
                    accumulated = accumulated.split_off(accumulated.len() - 365);
                    assert!(accumulated.len() == 365);
                }
                for (i, x) in accumulated.iter().enumerate() {
                    write!(data_str_buf, "({},{}),", i, x).unwrap();
                }

                let max = accumulated.to_owned().into_iter().reduce(Money::max).unwrap();
                let min = accumulated.into_iter().reduce(Money::min).unwrap();
                let percentage = max * dec!(100.0)/(max-min);
                let epsilon = dec!(1e-1);

                if percentage <= dec!(0.0) {
                    (
                        format!("({})", String::from_utf8(data_str_buf).unwrap()),
                        format!("(green.transparentize(100%), 0%), (green.transparentize(66%), 100%)"),
                        format!("(green, 0%), (green, 100%)")
                    )
                } else if percentage >= dec!(100.0) {
                    (
                        format!("({})", String::from_utf8(data_str_buf).unwrap()),
                        format!("(red.transparentize(33%), 0%), (red.transparentize(100%), 100%)"),
                        format!("(red, 0%), (red, 100%)")
                    )
                } else {
                    (
                        format!("({})", String::from_utf8(data_str_buf).unwrap()),
                        format!("(red.transparentize(33%), 0%), (red.transparentize(100%), {}%), (green.transparentize(100%), {}%), (green.transparentize(66%), 100%)", percentage - epsilon, percentage + epsilon),
                        format!("(red, 0%), (red, {}%), (green, {}%), (green, 100%)", percentage - epsilon, percentage + epsilon)
                    )
                }
            };

            writeln!(buf, "#v(1em)").unwrap();
            writeln!(buf, "#align(center,").unwrap();
            writeln!(buf, "cetz.canvas({{").unwrap();
            writeln!(buf, "import cetz.draw: *").unwrap();
            writeln!(buf, "import cetz-plot: *").unwrap();
            writeln!(buf).unwrap();
            writeln!(buf, "plot.plot(").unwrap();
            writeln!(buf, "    size: (15, 3),").unwrap();
            writeln!(buf, "    axis-style: none,").unwrap();
            writeln!(buf, "    {{").unwrap();
            writeln!(buf, "    plot.add(").unwrap();
            writeln!(buf, "        {},", data_str).unwrap();
            writeln!(buf, "        fill: true,").unwrap();
            writeln!(buf, "        style: (stroke: gradient.linear({}, dir: direction.ttb), fill: gradient.linear({}, dir: direction.ttb)),", stroke_gradient, fill_gradient).unwrap();
            writeln!(buf, "    )").unwrap();
            writeln!(buf, "    }}").unwrap();
            writeln!(buf, ")").unwrap();
            writeln!(buf, "}}))").unwrap();

        }
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
        let average_budget: Money = budget.accumulated_days(total_days)*dec!(365.0)/Decimal::from(total_days);
        let percentage =  average*dec!(100.0)/average_budget;
        let color = if percentage > dec!(100.0) { "red" } else if percentage > dec!(75.0) { "orange" } else { "green" };
        write!(buf, "#align(center, [#text([`{:.0}`], fill: {}) in average per 365 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.0}` _(budget)_\\ ", percentage, average_budget).unwrap();
        if percentage < dec!(95.0) {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.0}`], fill: {})!])])", budget.accumulated_days(total_days) - total, color).unwrap();
        } else if percentage > dec!(100.0) {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.0}`], fill: {})!])])", total - budget.accumulated_days(total_days), color).unwrap();
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
            let allowed = budget.accumulated_days_to(-days, year_start);
            if y_stats.total > allowed {
                writeln!(buf, "([{}], ({}, {})),", y, allowed, y_stats.total - allowed).unwrap();
            } else {
                writeln!(buf, "([{}], {}),", y, y_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (14, 8), bar-style: cetz.palette.new(colors: (black.lighten(85%), red.lighten(50%))), x-label: [Year], y-label: [Amount spent])").unwrap();
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
        let average_budget = budget.accumulated_days(total_days)*dec!(30.0)/Decimal::from(total_days);
        let percentage =  average*dec!(100.0)/average_budget;
        let color = if percentage > dec!(100.0) { "red" } else if percentage > dec!(75.0) { "orange" } else { "green" };
        write!(buf, "#align(center, [#text([`{:.0}`], fill: {}) in average per 30 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.0}` _(budget)_\\ ", percentage, average_budget).unwrap();
        if percentage < dec!(95.0) {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.0}`], fill: {})!])])", budget.accumulated_days(total_days) - total, color).unwrap();
        } else if percentage > dec!(100.0) {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.0}`], fill: {})!])])", total - budget.accumulated_days(total_days), color).unwrap();
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
            } else if stats.end.year() == *y && stats.start.month() == *m && today.year() == *y && today.month() == *m {
                days_in_month(month_start) - ((next_month(month_start) - today).num_days() - 1)
            } else {
                days_in_month(month_start)
            };
            let allowed = budget.accumulated_days_to(-n_days, month_start);
            if m_stats.total > allowed {
                writeln!(buf, "([{:02}/{}], ({}, {})),", m, y%100, allowed, m_stats.total - allowed).unwrap();
            } else {
                writeln!(buf, "([{:02}/{}], {}),", m, y%100, m_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (14, 7.5), bar-style: cetz.palette.new(colors: (black.lighten(85%), red.lighten(50%))), x-label: [Month], y-label: [Amount spent])").unwrap();
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
        let average_budget = budget.accumulated_days(total_days)*dec!(7.0)/Decimal::from(total_days);
        let percentage =  average*dec!(100.0)/average_budget;
        let color = if percentage > dec!(100.0) { "red" } else if percentage > dec!(75.0) { "orange" } else { "green" };
        write!(buf, "#align(center, [#text([`{:.0}`], fill: {}) in average per 7 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.0}` _(budget)_\\ ", percentage, average_budget).unwrap();
        if percentage < dec!(95.0) {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.0}`], fill: {})!])])", budget.accumulated_days(total_days) - total, color).unwrap();
        } else if percentage > dec!(100.0) {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.0}`], fill: {})!])])", total - budget.accumulated_days(total_days), color).unwrap();
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
        for (week, m_stats) in stats.weekly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            let week_start = NaiveDate::from_isoywd_opt(week.year(), week.week(), Weekday::Mon).unwrap();
            let allowed = if today.iso_week() == *week {
                budget.accumulated_days_to( -(today.signed_duration_since(week_start).num_days() + 1), week_start)
            } else {
                budget.accumulated_days_to( -7, week_start)
            };
            let label = if (week_start - TimeDelta::days(7)).month() != week_start.month() {
                format!("#underline[{:02}/{:02}]", week_start.day(), week_start.month())
            } else {
                format!("{:02}/{:02}", week_start.day(), week_start.month())
            };
            if m_stats.total > allowed {
                writeln!(buf, "(text(10pt, [{}]), ({}, {})),", label, allowed, m_stats.total - allowed).unwrap();
            } else {
                writeln!(buf, "(text(10pt,[{}]), {}),", label, m_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (12, 7), bar-style: cetz.palette.new(colors: (black.lighten(85%), red.lighten(50%))), x-label: [Week], y-label: [Amount spent])").unwrap();
        writeln!(buf, "}})]").unwrap();

    writeln!(buf, "").unwrap();
    writeln!(buf, "= Data").unwrap();
        writeln!(buf, "").unwrap();
        let mut ns =stats.last_n_days.keys().collect::<Vec<_>>();
        ns.sort();
        for n_days in ns {
            write_typ_table(&mut buf, stats, budget, *n_days);
        }

    writeln!(buf, "").unwrap();
    let mut f = std::fs::File::create(file_path).unwrap();
    f.write(buf.as_slice()).unwrap();
}

fn parse_raw_xml(file_path: &PathBuf) -> (Vec<RawBudget>, Vec<RawTransaction>) {
    let content = fs::read_to_string(file_path).unwrap_or_default();
    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);
    
    let mut budgets = Vec::new();
    let mut transactions = Vec::new();
    
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
                            budgets.push(RawBudget {
                                category,
                                amount,
                                duration,
                                date,
                            });
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
                            transactions.push(RawTransaction {
                                amount,
                                category,
                                date,
                                payment_method,
                                note: note.unwrap_or_default(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                println!("[ERROR]: XML parsing error `{}`", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    
    (budgets, transactions)
}

fn write_xml_file(file_path: &PathBuf, budgets: &[RawBudget], transactions: &[RawTransaction]) -> std::io::Result<()> {
    enum DBEntry<'a> {
        Budget(&'a RawBudget),
        Transaction(&'a RawTransaction),
    }
    use DBEntry::*;

    let entries = {
        let mut entries = Vec::with_capacity(budgets.len() + transactions.len());
        for b in budgets {
            entries.push(Budget(b));
        }
        for t in transactions {
            entries.push(Transaction(t));
        }
        entries.sort_by(|x,y|
            match (x,y) {
                (Budget(b1), Budget(b2)) => {
                    let b1_date = NaiveDate::parse_from_str(&b1.date, "%d/%m/%Y").unwrap();
                    let b2_date = NaiveDate::parse_from_str(&b2.date, "%d/%m/%Y").unwrap();
                    if b1_date == b2_date {
                            (b2.amount.parse::<Money>().unwrap()).partial_cmp(&b1.amount.parse::<Money>().unwrap()).unwrap()
                    } else {
                        b2_date.cmp(&b1_date)
                    }
                },
                (Transaction(t1), Transaction(t2)) => {
                    let t1_date = NaiveDate::parse_from_str(&t1.date, "%d/%m/%Y").unwrap();
                    let t2_date = NaiveDate::parse_from_str(&t2.date, "%d/%m/%Y").unwrap();
                    if t1_date == t2_date {
                            (t2.amount.parse::<Money>().unwrap()).partial_cmp(&t1.amount.parse::<Money>().unwrap()).unwrap()
                    } else {
                        t2_date.cmp(&t1_date)
                    }
                },
                (Budget(b), Transaction(t)) => {
                    let t_date = NaiveDate::parse_from_str(&t.date, "%d/%m/%Y").unwrap();
                    let b_date = NaiveDate::parse_from_str(&b.date, "%d/%m/%Y").unwrap();
                    if t_date == b_date {
                        Ordering::Greater
                    } else {
                        t_date.cmp(&b_date)
                    }
                },
                (Transaction(t),Budget(b)) => {
                    let t_date = NaiveDate::parse_from_str(&t.date, "%d/%m/%Y").unwrap();
                    let b_date = NaiveDate::parse_from_str(&b.date, "%d/%m/%Y").unwrap();
                    if t_date == b_date {
                        Ordering::Less
                    } else {
                        b_date.cmp(&t_date)
                    }
                }
            }
        );
        entries
    };

    let mut content = String::new();
    
    for e in entries {
        match e {
            Budget(budget) => {
                if let Some(ref category) = budget.category {
                    content.push_str(&format!(
                        "<budget category=\"{}\" amount=\"{}\" duration=\"{}\" date=\"{}\"/>\n",
                        category, budget.amount, budget.duration, budget.date
                    ));
                } else {
                    content.push_str(&format!(
                        "<budget amount=\"{}\" duration=\"{}\" date=\"{}\"/>\n",
                        budget.amount, budget.duration, budget.date
                    ));
                }   
            },
            Transaction(transaction) => {
                if transaction.note.is_empty() {
                    content.push_str(&format!(
                        "<transaction amount=\"{}\" category=\"{}\" date=\"{}\" payment-method=\"{}\">\n",
                        transaction.amount, transaction.category, transaction.date, transaction.payment_method
                    ));
                } else {
                    content.push_str(&format!(
                        "<transaction amount=\"{}\" category=\"{}\" date=\"{}\" payment-method=\"{}\" note=\"{}\">\n",
                        transaction.amount, transaction.category, transaction.date, transaction.payment_method, transaction.note
                    ));
                }
            }
        }
        
    }
    
    let mut sorted_transactions = transactions.to_vec();
    sorted_transactions.sort_by(|a, b| {
        let date_a = NaiveDate::parse_from_str(&a.date, "%d/%m/%Y")
            .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
        let date_b = NaiveDate::parse_from_str(&b.date, "%d/%m/%Y")
            .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
        date_b.cmp(&date_a)
    });
    
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
        }
        let cur_path = PathBuf::from(arg);
        match cur_path.try_exists() {
            Ok(true) => {
                path = Some(cur_path);
                break;
            }
            _ => {}
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
                    println!("[ERROR] Invalid date format. Please use dd/mm/yyyy.");
                    prompt_date_with_default(default)
                },
            };
            if NaiveDate::parse_from_str(&corrected_input, "%d/%m/%Y").is_ok() {
                corrected_input
            } else {
                println!("[ERROR] Invalid date format. Please use dd/mm/yyyy.");
                prompt_date_with_default(default)
            }
        } else {
            println!("[ERROR] Invalid date format. Please use dd/mm/yyyy.");
            prompt_date_with_default(default)
        }
    }
}

fn add_transactions_interactive(file_path: &PathBuf) -> std::io::Result<()> {
    fs::copy(file_path, format!("{}.bak", file_path.display())).unwrap();
    let (budgets, mut transactions) = parse_raw_xml(file_path);
    
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
        println!("{}\n", tclear());
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
        transactions.push(new_transaction);
        write_xml_file(file_path, &budgets, &transactions)?;
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
        return;
    }

    assert!(path.is_some(), "Rust has a problem here.");
    let path = path.unwrap();

    if add {
        match add_transactions_interactive(&path) {
            Ok(_) => println!("[INFO] Transaction addition completed."),
            Err(e) => eprintln!("[ERROR] Failed to add transactions: {}", e),
        }
    }

    let (transactions, budget) = parse_file(&path);

    if transactions.is_empty() {
        println!("[INFO] Provided file has no transactions. Exiting...");
        return;
    }

    let stats = get_stats(&transactions);
    
    let mut out_path = path.clone();
    out_path.set_extension("typ");
    write_typ_report(&out_path, &stats, &budget, &path);
    println!("[INFO] Detailed report saved in `{}`.", out_path.display());
}
