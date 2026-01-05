use std::{
    collections::HashMap, env, fmt::Debug, fs, io::Write, path::PathBuf, process::exit
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
            if (today - transaction.date).num_days() <= *i as i64 {
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
    writeln!(buf, "#show heading.where(level: 3): it => align(center, box(inset: (top: 2em, bottom: 0.25em), text(it, 16pt)))").unwrap();
    writeln!(buf, "#show heading.where(level: 2): it => align(center, box(inset: (top: 1em, bottom: 0.25em), text(it, 18pt)))").unwrap();
    writeln!(buf, "#show heading.where(level: 1): it => pagebreak() + align(center, box(inset: (top: 2em, bottom: 0.5em), text(it, 24pt)))").unwrap();
    writeln!(buf, "#set heading(numbering: \"1.\")").unwrap();
    writeln!(buf, "").unwrap();

    writeln!(buf, "#v(1fr)").unwrap();
    writeln!(buf, "#align(center, text([*Spending report from* `{}`], 28pt))", original_path.display()).unwrap();
    writeln!(buf, "#align(center, text([January 5, 2026], 20pt))").unwrap();
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
        let color = if average > budget.total * 30.0 { "red" } else if average / (budget.total*30.0) > 0.75 { "orange" } else { "green" };
        writeln!(buf, "#align(center, [#text([`{:.2}`], fill: {}) in average per 30 days\\ _{:.0}% of_ `{:.2}` _(budget)_])", average, color, average*100.0/(budget.total*30.0), budget.total * 30.0).unwrap();
        writeln!(buf).unwrap();
        writeln!(buf, "#v(1em)").unwrap();
        writeln!(buf).unwrap();
            
        writeln!(buf, "#align(center)[#cetz.canvas({{").unwrap();
        writeln!(buf, "import cetz.draw: *").unwrap();
        writeln!(buf, "import cetz-plot: *").unwrap();
        writeln!(buf, "chart.columnchart((").unwrap();
        for ((y, m), m_stats) in stats.monthly.iter().rev().zip(0..12).map(|x| x.0).rev() {
            let month_start = NaiveDate::from_ymd_opt(*y,*m, 1).unwrap();
            let allowed = days_in_month(month_start) as f64 * budget.total;
            if m_stats.total > allowed {
                writeln!(buf, "([{:02}/{}], ({}, {})),", m, y%100, allowed, m_stats.total - allowed).unwrap();
            } else {
                writeln!(buf, "([{:02}/{}], {}),", m, y%100, m_stats.total).unwrap();
            }
        }
        writeln!(buf, "), mode: \"stacked\", size: (auto, 7.5), bar-style: cetz.palette.new(colors: (black.lighten(85%), red.lighten(50%))), x-label: [Month], y-label: [Amount spent])").unwrap();
        writeln!(buf, "}})]").unwrap();
        writeln!(buf, "").unwrap();

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

    }

    let (transactions, budget) = parse_file(&path);

    if transactions.is_empty() {
        println!("[INFO] Provided file has no transactions. Exiting...");
        return;
    }

    let stats = get_stats(&transactions);
    // print_stats(&stats);

    let mut out_path = path.clone();
    out_path.set_extension("typ");
    write_typ_report(&out_path, &stats, &budget, &path);
    println!("Detailed report saved in `{}`.", out_path.display());
}
