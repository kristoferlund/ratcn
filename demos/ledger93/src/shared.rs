//! Domain data and display preferences shared by multiple screens.
//!
//! Settings writes [`Prefs`]. Ledger and Report read it when formatting amounts.

#[derive(Debug, Default)]
pub struct Shared {
    pub prefs: Prefs,
}

#[derive(Debug, Clone, Copy)]
pub struct Prefs {
    pub currency: Currency,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            currency: Currency::Usd,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PrefsMsg {
    SetCurrency(Currency),
}

impl Prefs {
    pub fn update(&mut self, msg: PrefsMsg) {
        match msg {
            PrefsMsg::SetCurrency(currency) => self.currency = currency,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Currency {
    #[default]
    Usd,
    Eur,
    Gbp,
    Jpy,
}

impl Currency {
    pub const ALL: [Currency; 4] = [Currency::Usd, Currency::Eur, Currency::Gbp, Currency::Jpy];

    pub const fn symbol(self) -> &'static str {
        match self {
            Currency::Usd => "$",
            Currency::Eur => "€",
            Currency::Gbp => "£",
            Currency::Jpy => "¥",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Eur => "EUR",
            Currency::Gbp => "GBP",
            Currency::Jpy => "JPY",
        }
    }

    /// A row label for the Settings currency picker, e.g. `"USD  $"`.
    pub const fn label(self) -> &'static str {
        match self {
            Currency::Usd => "USD  $",
            Currency::Eur => "EUR  €",
            Currency::Gbp => "GBP  £",
            Currency::Jpy => "JPY  ¥",
        }
    }
}

/// Where a ledger entry is booked. Income is tracked but not charted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Office,
    Hardware,
    Travel,
    Coffee,
    Software,
    Income,
}

impl Category {
    /// The categories the Report tab draws bars for (income is excluded).
    pub const EXPENSES: [Category; 5] = [
        Category::Office,
        Category::Hardware,
        Category::Travel,
        Category::Coffee,
        Category::Software,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Category::Office => "Office",
            Category::Hardware => "Hardware",
            Category::Travel => "Travel",
            Category::Coffee => "Coffee",
            Category::Software => "Software",
            Category::Income => "Income",
        }
    }
}

/// One booked transaction. `cents` is negative for an expense.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub label: &'static str,
    pub category: Category,
    pub cents: i64,
}

const fn entry(label: &'static str, category: Category, cents: i64) -> Entry {
    Entry {
        label,
        category,
        cents,
    }
}

/// The books. Immutable domain data, read by the Ledger and Report tabs alike.
pub const SEED: [Entry; 14] = [
    entry("Client invoice #0417", Category::Income, 1_250_000),
    entry("Client invoice #0418", Category::Income, 480_000),
    entry("Y2K audit software", Category::Software, -750_000),
    entry("Mainframe rental (Q2)", Category::Hardware, -125_000),
    entry("Espresso machine, deluxe", Category::Coffee, -95_000),
    entry("Spreadsheet license", Category::Software, -29_900),
    entry("Modem, 56k", Category::Hardware, -8_900),
    entry("Taxi to the server farm", Category::Travel, -76_200),
    entry("Floppy disks (bulk box)", Category::Hardware, -4_200),
    entry("Dot-matrix paper", Category::Office, -63_400),
    entry("Fax toner cartridge", Category::Office, -51_899),
    entry("Parking, downtown", Category::Travel, -21_500),
    entry("Stapler, heavy duty", Category::Office, -1_299),
    entry("Coffee, industrial tin", Category::Coffee, -5_000),
];

/// The net balance across the whole ledger.
pub fn balance() -> i64 {
    SEED.iter().map(|entry| entry.cents).sum()
}

/// Total spent in one expense category (a positive magnitude).
pub fn category_total(category: Category) -> i64 {
    SEED.iter()
        .filter(|entry| entry.category == category)
        .map(|entry| entry.cents.abs())
        .sum()
}

/// The one place money becomes text, so Ledger, Report, and Settings agree.
pub fn format_money(cents: i64, prefs: &Prefs) -> String {
    let negative = cents < 0;
    let magnitude = cents.unsigned_abs();

    let whole = magnitude / 100;
    let frac = magnitude % 100;
    let number = format!("{}.{:02}", group_thousands(whole), frac);

    let body = format!("{}{number}", prefs.currency.symbol());
    if negative { format!("({body})") } else { body }
}

fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let len = digits.len();
    let mut grouped = String::with_capacity(len + len / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (len - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped
}
