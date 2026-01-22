use core::fmt;
use std::{
    collections::HashMap, env, fmt::{Debug}, fs, io::{self, Write}, path::PathBuf, process::exit
};

use chrono::{Datelike, Local, NaiveDate, TimeDelta};
use quick_xml::{Reader, events::Event};

const LAST_N_DAYS: [u64; 4] = [7, 14, 30, 365];

type Category = String;

#[derive(Debug, Default)]
struct Budget {
    per_category: HashMap<Category, f64>,
    total: f64,
}

#[derive(Debug, Default)]
struct Transaction {
    value: i64, // units and cents
    date: NaiveDate,
    category: Category,
    payment_method: String,
    note: String,
}

#[derive(Debug, Default)]
struct Stats {
    #[allow(unused)]
    per_day: f64,
    total: f64,
    by_category: Vec<(Category, f64)>,
    #[allow(unused)]
    by_payment_method: Vec<(String, f64)>,
    #[allow(unused)]
    by_note: Vec<(String, f64)>,
    #[allow(unused)]
    average_transaction: f64,
    #[allow(unused)]
    transaction_count: u64,
}

#[derive(Debug, Default)]
struct TempStats {
    per_day: f64,
    total: i64,
    by_category: HashMap<Category, i64>,
    by_payment_method: HashMap<String, i64>,
    by_note: HashMap<String, i64>,
    average_transaction: f64,
    transaction_count: u64,
}

impl TempStats {
    pub fn update(&mut self, e: &Transaction) {
        let value = e.value;
        self.total += value;
        if !self.by_category.contains_key(&e.category) {
            self.by_category.insert(e.category.clone(), 0);
        }
        *(self.by_category.get_mut(&e.category).unwrap()) += value;

        if !self.by_payment_method.contains_key(&e.payment_method) {
            self.by_payment_method.insert(e.payment_method.clone(), 0);
        }
        *(self.by_payment_method.get_mut(&e.payment_method).unwrap()) += value;

        if !self.by_note.contains_key(&e.note) {
            self.by_note.insert(e.note.clone(), 0);
        }
        *(self.by_note.get_mut(&e.note).unwrap()) += value;

        self.transaction_count += 1;
    }

    pub fn calc_averages(&mut self, days: u64) {
        let days = days as f64;
        self.per_day = self.get_total() / days;
        self.average_transaction = self.get_total() / self.transaction_count as f64;
    }

    pub fn into_stats(self) -> Stats {
        let mut by_category = self.by_category.into_iter().map(|(k,v)| (k, v as f64/100.0)).collect::<Vec<_>>();
        by_category.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_payment_method = self.by_payment_method.into_iter().map(|(k,v)| (k, v as f64/100.0)).collect::<Vec<_>>();
        by_payment_method.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_note = self.by_note.into_iter().map(|(k,v)| (k, v as f64/100.0)).collect::<Vec<_>>();
        by_note.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        Stats {
            per_day: self.per_day,
            total: self.total as f64 / 100.0,
            by_category,
            by_payment_method,
            by_note,
            average_transaction: self.average_transaction,
            transaction_count: self.transaction_count,
        }
    }

    fn get_total(&self) -> f64 {
        self.total as f64 / 100.0
    }
}

#[derive(Debug, Default)]
struct StatsCollection {
    #[allow(unused)]
    yearly: Vec<(i32, Stats)>,         // year
    monthly: Vec<((i32, u32), Stats)>, // year, month
    last_n_days: HashMap<u64, Stats>
}

#[derive(Debug, Default)]
struct TempStatsCollection {
    yearly: HashMap<i32, TempStats>,         // year
    monthly: HashMap<(i32, u32), TempStats>, // year, month
    last_n_days: HashMap<u64, TempStats>
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
        StatsCollection {
            yearly: yearly,
            monthly: monthly,
            last_n_days: self.last_n_days.into_iter().map(|(x,y)| (x, y.into_stats())).collect(),
        }
    }
}

// struct DateRange(NaiveDate, NaiveDate);

// impl Iterator for DateRange {
//     type Item = NaiveDate;

//     fn next(&mut self) -> Option<Self::Item> {
//         if self.0 <= self.1 {
//             let next = self.0 + Duration::days(1);
//             Some(mem::replace(&mut self.0, next))
//         } else {
//             None
//         }
//     }
// }

#[allow(dead_code)]
fn moving_average(xs: Vec<f64>, window: isize) -> Vec<f64> {
    let mut average = Vec::new();

    for i in 0..xs.len() {
        let mut a = 0.0;
        let start = (i as isize - window + 1).max(0) as usize;
        let n = (i - start + 1) as f64;
        for j in start..=i {
            a += xs[j];
        }
        a /= n;
        average.push(a);
    }
    assert!(average.len() == xs.len());
    return average;
}

#[allow(dead_code)]
fn weighted_moving_average(xs: Vec<(f64, f64)>, window: isize) -> Vec<f64> {
    let mut average = Vec::new();

    for i in 0..xs.len() {
        let mut a = 0.0;
        let mut d = 0.0;
        let start = (i as isize - window + 1).max(0) as usize;
        for j in start..=i {
            a += xs[j].0 * xs[j].1;
            d += xs[j].1;
        }
        a /= d;
        average.push(a);
    }
    assert!(average.len() == xs.len());
    return average;
}

fn days_in_month(d: NaiveDate) -> i64 {
    let year = year_as_i32(d.year_ce());
    let month = d.month0() + 1;
    (NaiveDate::from_ymd_opt(year + if month == 12 { 1 } else { 0 }, (month % 12) + 1, 1).unwrap()
        - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days()
}

fn days_in_year(d: NaiveDate) -> i64 {
    let year = year_as_i32(d.year_ce());
    let month = d.month0() + 1;
    (NaiveDate::from_ymd_opt(year + 1, month, 1).unwrap()
        - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days()
}

fn year_as_i32(year_ce: (bool, u32)) -> i32 {
    if year_ce.0 {
        year_ce.1 as i32
    } else {
        -1 * year_ce.1 as i32
    }
}

fn parse_amount_as_cents(field: &str) -> i64 {
    let negative = field.trim().starts_with('-');
    let mut parts = field.split('.');
    let units = parts.next().unwrap().trim().parse::<i32>().unwrap();
    let cents = parts
        .next()
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .unwrap_or(0);

    if cents >= 100 {
        eprintln!(
            "[ERROR] Could not parse amount `{}` (cents seem to have too many digits).",
            field.trim(),
        );
        exit(1);
    }
    let cents = if units < 0 || negative {
        -(cents as i64)
    } else {
        cents as i64
    } * if cents < 10 { 10 } else { 1 };
    return units as i64 * 100 + cents;
}

fn print_usage() {
    println!("USAGE: {} <path/to/file.xml>", env::args().next().unwrap());
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

fn parse_file(filepath: &PathBuf) -> (Vec<Transaction>, Budget) {
    let content = fs::read_to_string(&filepath).unwrap_or_default();
    let mut transactions  = Vec::new();
    let mut budget = Budget::default();

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
                        assert!(!budget.per_category.contains_key(category));
                        budget.per_category.insert(category.to_owned(), attributes.get("amount").unwrap().parse::<f64>().unwrap() / attributes.get("duration").unwrap().parse::<f64>().unwrap());
                    } else {
                        budget.total = attributes.get("amount").unwrap().parse::<f64>().unwrap() / attributes.get("duration").unwrap().parse::<f64>().unwrap();
                    }
                },
                "transaction" => {
                    let attributes =  e.attributes().map(|x| {
                        let x = x.unwrap();
                        (String::from_utf8(x.key.as_ref().to_vec()).unwrap(), String::from_utf8(x.value.as_ref().to_vec()).unwrap())
                    }).collect::<HashMap<_,_>>();

                    transactions.push(Transaction {
                        category: attributes.get("category").unwrap().trim().to_owned(),
                        date: NaiveDate::parse_from_str(attributes.get("date").unwrap().trim(), "%d/%m/%Y").unwrap(),
                        value: parse_amount_as_cents(attributes.get("amount").unwrap()),
                        note: if attributes.contains_key("note") { attributes.get("note").unwrap().to_owned() } else { String::default() },
                        payment_method: attributes.get("payment-method").unwrap().trim().to_owned(),
                    });
                },
                x => {
                    println!("[ERROR]: Unknown tag: `{}`", x);
                },
            },
            Ok(_) => {},
            Err(e) => println!("[ERROR]: XML parsin error `{}`", e),
        }
    }

    if budget.total == 0.0 {
        budget.total = budget.per_category.values().sum();
    }

    return (transactions, budget);
}

fn get_stats(transactions: &Vec<Transaction>) -> StatsCollection {
    let mut tsc = TempStatsCollection::default();
    let today = Local::now().date_naive();

    let mut start = today;
    for transaction in transactions.iter() {
        let year = year_as_i32(transaction.date.year_ce());
        let month = transaction.date.month0() + 1;
        start = start.min(transaction.date);

        // Yearly
        if !tsc.yearly.contains_key(&year) {
            tsc.yearly.insert(year, TempStats::default());
        }
        tsc.yearly.get_mut(&year).unwrap().update(transaction);

        // Monthly
        let month_idx = (year, month);
        if !tsc.monthly.contains_key(&month_idx) {
            tsc.monthly.insert(month_idx, TempStats::default());
        }
        tsc.monthly.get_mut(&month_idx).unwrap().update(transaction);

        for i in LAST_N_DAYS.iter() {
            if (today - transaction.date).num_days() < *i as i64 {
                if !tsc.last_n_days.contains_key(i) {
                    tsc.last_n_days.insert(*i, TempStats::default());
                }
                tsc.last_n_days.get_mut(&i).unwrap().update(transaction);
            }
        }
    }

    for (k, v) in tsc.yearly.iter_mut() {
        let year_start = NaiveDate::from_ymd_opt(*k, 1, 1).unwrap();
        let period_start = year_start.max(start);
        let period_end = (NaiveDate::from_ymd_opt(*k + 1, 1, 1).unwrap() - TimeDelta::days(1))
            .min(today + TimeDelta::days(1));
        let days = days_in_year(year_start);
        let days2 = (period_end - period_start).num_days();
        // println!("{} {} {} {} {}", year_start, period_start, period_end, days, days2);
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
        let days2 = (period_end - period_start).num_days();
        // println!("{} {} {} {} {} {}", month_start, month_end, period_start, period_end, days, days2);
        let ndays = days.min(days2);
        assert!(ndays > 0);
        v.calc_averages(ndays as u64);
    }

    let ns = tsc.last_n_days.keys().map(|x| *x).collect::<Vec<_>>();
    for n in ns.into_iter() {
        tsc.last_n_days.get_mut(&n).unwrap().calc_averages(n);
    }

    return tsc.into_stats_collection();
}

fn write_typ_table(buf: &mut Vec<u8>, stats: &StatsCollection, budget: &Budget, n_days: u64) {
    let stats = stats.last_n_days.get(&n_days).unwrap();
    writeln!(buf, "== Last {} days", n_days).unwrap();
        writeln!(buf, "").unwrap();
        writeln!(buf, "#align(center, table(columns: 4, align: left, stroke: 0pt, column-gutter: 5pt, table.hline(stroke: 1pt), [*Category*], [*Amount*], [*% of Total*], [*Allowed spending*],").unwrap();
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        for (category, amount) in stats.by_category.iter() {
            write!(buf, "    [{}], align(right, [`{:.2}`]), align(right, [`{:.2}%`]),", category, amount, (*amount / stats.total) * 100.0).unwrap();
            let allowed_amount = if let Some(allowed_amount) = budget.per_category.get(category) {
                if budget.total*n_days as f64 > stats.total {
                    let allowed = allowed_amount*n_days as f64 - *amount;
                    (format!("{:.2}", allowed), if allowed >= 0.0  {
                        if allowed / allowed_amount >= 0.25 {
                            "green"
                        } else {
                            "orange"
                        }
                    } else {
                        "red"
                    })
                } else {
                    (String::default(), "black")
                }
            } else {
                (String::default(), "black")
            };
            write!(buf, "align(right, text([`{}`], fill: {})), ", allowed_amount.0, allowed_amount.1).unwrap();
            writeln!(buf).unwrap();
            writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap()
        }
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        let allowed_amount =if budget.total > 0.0 {
                let allowed = budget.total*n_days as f64 - stats.total;
                (format!("{:.2}", allowed), if allowed >= 0.0  {
                    if allowed / budget.total >= 0.25 {
                        "green"
                    } else {
                        "orange"
                    }
                } else {
                    "red"
                })

            } else {
                (String::default(), "black")
            };
        writeln!(buf, "    [*Total*], align(right, [`{:.2}`]), align(right, [`{:.2}%`]), align(right, text([`{}`], fill: {})), ", stats.total as f64, 100.0, allowed_amount.0, allowed_amount.1).unwrap();
        writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
        writeln!(buf, "))").unwrap();
        writeln!(buf, "").unwrap();
        if n_days > 31 {
            writeln!(buf, "#v(2em)").unwrap();
            writeln!(buf, "").unwrap();
            writeln!(buf, "=== Biggest expenses (last {} days)", n_days).unwrap();
            writeln!(buf, "#align(center, table(columns: 3, stroke: 0pt, align: (right, left, right), ").unwrap();
            for ((note, amount),i) in stats.by_note.iter().filter(|x| x.1 > 50.0).zip(0..10) {
                writeln!(buf, "[{}.], [_\"{}\"_], [`{:.2}`], ", i+1, note, amount).unwrap();
            }
            writeln!(buf, "))").unwrap();
        }
}

fn write_typ_report(file_path: &PathBuf, stats: &StatsCollection, budget: &Budget, original_path: &PathBuf) {
    let today = Local::now().date_naive();

    let mut buf = Vec::new();
    writeln!(buf, "#import \"@preview/cetz:0.3.2\"").unwrap();
    writeln!(buf, "#import \"@preview/cetz-plot:0.1.1\"").unwrap();
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

    writeln!(buf, "= Monthly Budget").unwrap();
    let mut budget_categories = budget.per_category.keys().collect::<Vec<_>>();
    budget_categories.sort();
    writeln!(buf, "#align(center, table(columns: 3, stroke: 0pt, align: (left, right, right), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Category*], align(left, [*Allowed amount*]), align(left, [*% of Total*]), ").unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    for category in budget_categories {
        writeln!(buf, "[{}], [`{:.2}`], [`{:.0}%`],", category, budget.per_category.get(category).unwrap() * 30.0, (budget.per_category.get(category).unwrap() / budget.total)*100.0).unwrap();
    writeln!(buf, "    table.hline(stroke: 0.5pt),").unwrap();
    }
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "[*Total*], [`{:.2}`], ", budget.total * 30.0).unwrap();
    writeln!(buf, "    table.hline(stroke: 1pt),").unwrap();
    writeln!(buf, "))").unwrap();


    writeln!(buf, "").unwrap();
    writeln!(buf, "= 12 Month Overview").unwrap();
    writeln!(buf, "").unwrap();
        let mut total = 0.0;
        let mut total_days = 0.0;
        for ((y, m), m_stats) in stats.monthly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            let month_start = NaiveDate::from_ymd_opt(*y,*m, 1).unwrap();
            total += m_stats.total;
            total_days += days_in_month(month_start) as f64;
        }
    
        let average = total * 30.0 / total_days;
        let percentage =  average*100.0/(budget.total*30.0);
        let color = if percentage > 100.0 { "red" } else if percentage > 75.0 { "orange" } else { "green" };
        write!(buf, "#align(center, [#text([`{:.2}`], fill: {}) in average per 30 days\\ ", average, color).unwrap();
        write!(buf, "_{:.0}% of_ `{:.2}` _(budget)_\\ ", percentage, budget.total * 30.0).unwrap();
        if percentage < 95.0 {
            writeln!(buf, "#text(8pt, [You saved #text([`{:.2}`], fill: {})!])])", budget.total * total_days - total, color).unwrap();
        } else if percentage > 100.0 {
            writeln!(buf, "#text(8pt, [You lost #text([`{:.2}`], fill: {})!])])", total - budget.total * total_days, color).unwrap();
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
            let month_start = NaiveDate::from_ymd_opt(*y,*m, 1).unwrap();
            let allowed = if today.month() == month_start.month() && today.year() == month_start.year() {
                (today.signed_duration_since(month_start).num_days() + 1) as f64 * budget.total
            } else {
                days_in_month(month_start) as f64 * budget.total
            };
            if m_stats.total > allowed {
                writeln!(buf, "([{:02}/{}], ({}, {})),", m, y%100, allowed, m_stats.total - allowed).unwrap();
            } else {
                writeln!(buf, "([{:02}/{}], {}),", m, y%100, m_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (auto, 7.5), bar-style: cetz.palette.new(colors: (black.lighten(85%), red.lighten(50%))), x-label: [Month], y-label: [Amount spent])").unwrap();
        writeln!(buf, "}})]").unwrap();

    // writeln!(buf, "").unwrap();
    // writeln!(buf, "= 5 Year Overview").unwrap();
    // writeln!(buf, "").unwrap();
    //     let mut total = 0.0;
    //     let mut total_days = 0.0;
    //     for (y, y_stats) in stats.yearly.iter().rev().zip(0..5).map(|x| x.0).rev() {
    //         if NaiveDate::from_ymd_opt(*y,1, 1).unwrap().leap_year() {
    //             total_days += 366.0;
    //         } else {
    //             total_days += 365.0;
    //         }
    //         total += y_stats.total;
    //     }
    
    //     let average = total * 365.0 / total_days;
    //     let color = if average > budget.total * 365.0 { "red" } else if average / (budget.total*365.0) > 0.75 { "orange" } else { "green" };
    //     writeln!(buf, "#align(center, [#text([`{:.2}`], fill: {}) in average per 365 days\\ _{:.0}% of_ `{:.2}` _(budget)_])", average, color, average*100.0/(budget.total*365.0), budget.total * 365.0).unwrap();
    //     writeln!(buf).unwrap();
    //     writeln!(buf, "#v(1em)").unwrap();
    //     writeln!(buf).unwrap();
            
    //     writeln!(buf, "#align(center)[#cetz.canvas({{").unwrap();
    //     writeln!(buf, "import cetz.draw: *").unwrap();
    //     writeln!(buf, "import cetz-plot: *").unwrap();
    //     writeln!(buf, "chart.columnchart((").unwrap();
    //     for (y, y_stats) in stats.yearly.iter().rev().zip(0..5).map(|x| x.0).rev() {
    //         let allowed = if NaiveDate::from_ymd_opt(*y,1, 1).unwrap().leap_year() {
    //                 366.0
    //             } else {
    //                 365.0
    //             } * budget.total;
    //         if y_stats.total > allowed {
    //             writeln!(buf, "([{:04}], ({}, {})),", y, allowed, y_stats.total - allowed).unwrap();
    //         } else {
    //             writeln!(buf, "([{:04}], {}),", y, y_stats.total).unwrap();
    //         }
    //     }
    //     writeln!(buf, "), mode: \"stacked\", size: (auto, 7.5), bar-style: cetz.palette.new(colors: (black.lighten(85%), red.lighten(50%))), x-label: [Year], y-label: [Amount spent])").unwrap();
    //     writeln!(buf, "}})]").unwrap();
    //     writeln!(buf, "").unwrap();

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


#[derive(Debug, Clone)]
struct RawBudget {
    category: Option<String>,
    amount: String,  // Keep as string for exact preservation
    duration: String,
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
                        
                        for attr in e.attributes() {
                            let attr = attr.unwrap();
                            let key = String::from_utf8_lossy(&attr.key.0).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            
                            match key.as_str() {
                                "category" => category = Some(value),
                                "amount" => amount = Some(value),
                                "duration" => duration = Some(value),
                                _ => {}
                            }
                        }
                        
                        if let (Some(amount), Some(duration)) = (amount, duration) {
                            budgets.push(RawBudget {
                                category,
                                amount,
                                duration,
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
                eprintln!("Error parsing XML: {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    
    (budgets, transactions)
}

fn prompt_with_default(prompt: &str, default: &str) -> String {
    print!("{} [{}] > ", prompt, default);
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
    print!("Date [{}] (or 'today') > ", default);
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_string();
    
    if input.is_empty() {
        default.to_string()
    } else if input.to_lowercase() == "today" {
        Local::now().date_naive().format("%d/%m/%Y").to_string()
    } else {
        // Validate date format
        if NaiveDate::parse_from_str(&input, "%d/%m/%Y").is_ok() {
            input
        } else if input.split('/').all(|x| x.parse::<u32>().is_ok()) {
            let numbers = input.split('/').map(|x| x.parse::<u32>().unwrap()).collect::<Vec<_>>();
            let corrected_input = match numbers.len() {
                1 => format!("{:02}/{:02}/{}", input.parse::<u32>().unwrap(), default_date.month(), default_date.year()),
                2 => format!("{:02}/{:02}/{}", numbers[0], numbers[1], default_date.year()),
                _ =>  {
                    println!("Invalid date format. Please use dd/mm/yyyy.");
                    prompt_date_with_default(default)
                },
            };
            if NaiveDate::parse_from_str(&corrected_input, "%d/%m/%Y").is_ok() {
                corrected_input
            } else {
                println!("Invalid date format. Please use dd/mm/yyyy.");
                prompt_date_with_default(default)
            }
        } else {
            println!("Invalid date format. Please use dd/mm/yyyy.");
            prompt_date_with_default(default)
        }
    }
}

fn validate_amount(amount: &str) -> bool {
    let amount_str = amount.trim();
    if amount_str.is_empty() {
        return false;
    }
    
    // Try to parse as float
    if let Ok(parsed) = amount_str.parse::<f64>() {
        // Additional validation: check if it has reasonable precision
        parsed.is_finite()
    } else {
        false
    }
}

fn write_xml_file(file_path: &PathBuf, budgets: &[RawBudget], transactions: &[RawTransaction]) -> std::io::Result<()> {
    let mut content = String::new();
    
    // Sort budgets: total first (without category), then by amount descending
    let mut sorted_budgets = budgets.to_vec();
    sorted_budgets.sort_by(|a, b| {
        match (&a.category, &b.category) {
            (None, None) => {
                // Both are total budgets, sort by amount
                let a_amount = a.amount.parse::<f64>().unwrap_or(0.0);
                let b_amount = b.amount.parse::<f64>().unwrap_or(0.0);
                b_amount.partial_cmp(&a_amount).unwrap()
            }
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(_), Some(_)) => {
                // Both have categories, sort by amount
                let a_amount = a.amount.parse::<f64>().unwrap_or(0.0);
                let b_amount = b.amount.parse::<f64>().unwrap_or(0.0);
                b_amount.partial_cmp(&a_amount).unwrap()
            }
        }
    });
    
    // Write budget lines
    for budget in &sorted_budgets {
        if let Some(ref category) = budget.category {
            content.push_str(&format!(
                "<budget category=\"{}\" amount=\"{}\" duration=\"{}\"/>\n",
                category, budget.amount, budget.duration
            ));
        } else {
            content.push_str(&format!(
                "<budget amount=\"{}\" duration=\"{}\"/>\n",
                budget.amount, budget.duration
            ));
        }
    }
    
    // Sort transactions by date descending (newest first)
    let mut sorted_transactions = transactions.to_vec();
    sorted_transactions.sort_by(|a, b| {
        // Parse dates for comparison
        let date_a = NaiveDate::parse_from_str(&a.date, "%d/%m/%Y")
            .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
        let date_b = NaiveDate::parse_from_str(&b.date, "%d/%m/%Y")
            .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
        date_b.cmp(&date_a) // Reverse order for descending
    });
    
    // Write transaction lines
    for transaction in &sorted_transactions {
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
    
    fs::write(file_path, content)
}

fn add_transactions_interactive(file_path: &PathBuf) -> std::io::Result<()> {
    fs::copy(file_path, format!("{}.bak", file_path.display())).unwrap();

    println!("=== Transaction Entry Mode ===\n");
    
    // Parse existing file using quick_xml
    let (budgets, mut transactions) = parse_raw_xml(file_path);
    
    // Determine default values from last transaction
    let (mut default_date, mut default_category, mut default_payment_method) = if !transactions.is_empty() {
        // Sort transactions to get the most recent
        let mut sorted_transactions = transactions.clone();
        sorted_transactions.sort_by(|a, b| {
            let date_a = NaiveDate::parse_from_str(&a.date, "%d/%m/%Y")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
            let date_b = NaiveDate::parse_from_str(&b.date, "%d/%m/%Y")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1900, 1, 1).unwrap());
            date_b.cmp(&date_a) // Get most recent
        });
        
        let last = &sorted_transactions[0];
        (
            last.date.clone(),
            last.category.clone(),
            last.payment_method.clone()
        )
    } else {
        // No transactions yet, use today's date and empty strings
        (
            Local::now().date_naive().format("%d/%m/%Y").to_string(),
            String::new(),
            String::new()
        )
    };
    
    // Get list of existing categories for reference
    let existing_categories: Vec<String> = transactions.iter()
        .map(|t| t.category.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    
    if !existing_categories.is_empty() {
        println!("Existing categories: {}.", existing_categories.join(", "));
    }
    
    // Show existing budget categories
    let budget_categories: Vec<&str> = budgets.iter()
        .filter_map(|b| b.category.as_ref().map(|s| s.as_str()))
        .collect();
    
    if !budget_categories.is_empty() {
        println!("Budget categories: {}.", budget_categories.join(", "));
    }

    let last_date = transactions.iter().map(|x| NaiveDate::parse_from_str(&x.date, "%d/%m/%Y").unwrap()).max().unwrap();
    let last_transactions =  transactions.iter().filter(|x| NaiveDate::parse_from_str(&x.date, "%d/%m/%Y").unwrap() == last_date).collect::<Vec<_>>();

    if !last_transactions.is_empty() {
        println!("Last transactions:");
        for t in last_transactions.iter() {
            println!(" - {}", t);
        }
    }
    
    let mut loop_count = 0;
    
    loop {
        loop_count += 1;
        println!("\n--- Transaction #{} ---", loop_count);
        
        // Collect transaction details
        let date = prompt_date_with_default(&default_date);
        
        let category = if default_category.is_empty() {
            print!("Category > ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            input.trim().to_string()
        } else {
            prompt_with_default("Category", &default_category)
        };
        
        let amount = loop {
            print!("Amount > ");
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
        
        let payment_method = if default_payment_method.is_empty() {
            print!("Payment Method > ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            input.trim().to_string()
        } else {
            prompt_with_default("Payment Method", &default_payment_method)
        };
        
        print!("Note [optional] > ");
        io::stdout().flush().unwrap();
        let mut note = String::new();
        io::stdin().read_line(&mut note).unwrap();
        let note = note.trim().to_string();


        default_category = category.clone();
        default_date = date.clone();
        default_payment_method = payment_method.clone();
        
        // Create and add the transaction
        let new_transaction = RawTransaction {
            amount,
            category: category.clone(),
            date: date.clone(),
            payment_method: payment_method.clone(),
            note,
        };
        
        println!("{}", new_transaction);
        transactions.push(new_transaction);
        println!("✓ Transaction added!");
        
        // Ask if user wants to continue
        print!("Add another transaction? (y/n) > ");
        io::stdout().flush().unwrap();
        let mut response = String::new();
        io::stdin().read_line(&mut response).unwrap();
        
        let continue_condition = response.trim().to_lowercase().starts_with("y") || response.trim().is_empty();
        if !continue_condition {
            break;
        }
    }
    
    // Write updated XML back to file
    write_xml_file(file_path, &budgets, &transactions)?;
    
    println!("Saved {} transaction(s) to {}", loop_count, file_path.display());
    
    Ok(())
}

// Update the main function to use the new function
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
        // Call the interactive transaction addition function
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
    println!("Detailed report saved in `{}`.", out_path.display());
}


// fn main() {
//     let (path, add) = get_options();

//     if path.is_none() {
//         eprintln!("[ERROR] No file provided.");
//         print_usage();
//         return;
//     }

//     assert!(path.is_some(), "Rust has a problem here.");
//     let path = path.unwrap();

//     if add {

//     }

//     let (transactions, budget) = parse_file(&path);

//     if transactions.is_empty() {
//         println!("[INFO] Provided file has no transactions. Exiting...");
//         return;
//     }

//     let stats = get_stats(&transactions);
//     // print_stats(&stats);

//     let mut out_path = path.clone();
//     out_path.set_extension("typ");
//     write_typ_report(&out_path, &stats, &budget, &path);
//     println!("Detailed report saved in `{}`.", out_path.display());
// }
