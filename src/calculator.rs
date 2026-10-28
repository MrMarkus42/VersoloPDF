//! "Rechner": a desk calculator with a paper tape whose calculation can be
//! placed on a page as a text box (ribbon tab "Anmerkungen").

/// Operation shown next to a value on the tape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    /// Result of "=".
    Total,
    /// "ST": intermediate result, the calculation goes on.
    Subtotal,
    /// "GT": sum of all results.
    GrandTotal,
    Percent,
    Sqrt,
    Recip,
    TaxPlus,
    TaxMinus,
}

impl Op {
    /// Symbol on the tape; only characters the standard PDF fonts have.
    pub fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "×",
            Op::Div => "÷",
            Op::Total => "=",
            Op::Subtotal => "ST",
            Op::GrandTotal => "GT",
            Op::Percent => "%",
            Op::Sqrt => "sqrt",
            Op::Recip => "1/x",
            Op::TaxPlus => "+T",
            Op::TaxMinus => "-T",
        }
    }
}

/// One line of the tape.
#[derive(Debug, Clone, PartialEq)]
pub struct TapeLine {
    pub value: f64,
    pub op: Op,
}

/// Keys of the calculator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Digit(u8),
    DoubleZero,
    Point,
    Backspace,
    /// "CE": clears the current entry.
    ClearEntry,
    /// "C": clears the calculation.
    Clear,
    Negate,
    Add,
    Sub,
    Mul,
    Div,
    Equals,
    Percent,
    Sqrt,
    Recip,
    MemoryRecall,
    MemoryClear,
    MemoryStore,
    MemoryAdd,
    MemorySub,
    TaxPlus,
    TaxMinus,
    GrandTotal,
    Subtotal,
}

/// Longest entry in digits.
const MAX_DIGITS: usize = 15;
/// Longest tape; older lines are dropped.
const MAX_TAPE: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Calculator {
    /// Digits being typed; empty when the display shows `value`.
    entry: String,
    /// Value shown when nothing is typed.
    value: f64,
    acc: f64,
    pending: Option<Op>,
    pub memory: f64,
    pub grand_total: f64,
    /// Tax rate in percent for TAX+ / TAX−.
    pub tax_rate: f64,
    pub tape: Vec<TapeLine>,
    /// Lines removed with "Rückgängig" (for "Wiederholen").
    redo: Vec<TapeLine>,
    pub error: bool,
}

impl Default for Calculator {
    fn default() -> Self {
        Self {
            entry: String::new(),
            value: 0.0,
            acc: 0.0,
            pending: None,
            memory: 0.0,
            grand_total: 0.0,
            tax_rate: 19.0,
            tape: Vec::new(),
            redo: Vec::new(),
            error: false,
        }
    }
}

fn apply(op: Op, a: f64, b: f64) -> f64 {
    match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        Op::Div => a / b,
        _ => b,
    }
}

impl Calculator {
    /// The number currently shown.
    pub fn current(&self) -> f64 {
        if self.entry.is_empty() { self.value } else { self.entry.parse().unwrap_or(0.0) }
    }

    /// Display text with `decimals` decimal places (more while typing).
    pub fn display(&self, decimal_comma: bool) -> String {
        if self.error {
            return "E".into();
        }
        let text = if self.entry.is_empty() { format_number(self.value, 2) } else { self.entry.clone() };
        if decimal_comma { text.replace('.', ",") } else { text }
    }

    fn push(&mut self, value: f64, op: Op) {
        self.tape.push(TapeLine { value, op });
        self.redo.clear();
        if self.tape.len() > MAX_TAPE {
            self.tape.remove(0);
        }
    }

    /// Takes the current number and ends typing.
    fn take(&mut self) -> f64 {
        let v = self.current();
        self.entry.clear();
        self.value = v;
        v
    }

    fn set_result(&mut self, v: f64) {
        if v.is_finite() {
            self.value = v;
        } else {
            self.error = true;
            self.value = 0.0;
        }
    }

    pub fn press(&mut self, key: Key) {
        if self.error && !matches!(key, Key::Clear | Key::ClearEntry) {
            return;
        }
        match key {
            Key::Digit(d) if d <= 9 => self.type_digits(&d.to_string()),
            Key::Digit(_) => {}
            Key::DoubleZero => self.type_digits("00"),
            Key::Point => {
                if !self.entry.contains('.') {
                    if self.entry.is_empty() || self.entry == "-" {
                        self.entry.push('0');
                    }
                    self.entry.push('.');
                }
            }
            Key::Backspace => {
                self.entry.pop();
                if self.entry == "-" {
                    self.entry.clear();
                }
            }
            Key::ClearEntry => {
                self.entry.clear();
                self.value = 0.0;
                self.error = false;
            }
            Key::Clear => {
                self.entry.clear();
                self.value = 0.0;
                self.acc = 0.0;
                self.pending = None;
                self.error = false;
            }
            Key::Negate => {
                if self.entry.is_empty() {
                    self.value = -self.value;
                } else if let Some(rest) = self.entry.strip_prefix('-') {
                    self.entry = rest.to_string();
                } else {
                    self.entry.insert(0, '-');
                }
            }
            Key::Add | Key::Sub | Key::Mul | Key::Div => {
                let op = match key {
                    Key::Add => Op::Add,
                    Key::Sub => Op::Sub,
                    Key::Mul => Op::Mul,
                    _ => Op::Div,
                };
                let v = self.take();
                self.push(v, op);
                self.acc = match self.pending {
                    Some(p) => apply(p, self.acc, v),
                    None => v,
                };
                self.pending = Some(op);
                self.set_result(self.acc);
            }
            Key::Equals => self.finish(Op::Total),
            Key::Subtotal => {
                let v = self.take();
                let r = match self.pending {
                    Some(p) => apply(p, self.acc, v),
                    None => v,
                };
                self.push(r, Op::Subtotal);
                self.set_result(r);
            }
            Key::Percent => {
                let v = self.take();
                let pct = match self.pending {
                    Some(Op::Add | Op::Sub) => self.acc * v / 100.0,
                    _ => v / 100.0,
                };
                self.push(v, Op::Percent);
                self.value = pct;
                self.finish(Op::Total);
            }
            Key::Sqrt | Key::Recip => {
                let v = self.take();
                let (op, r) = if key == Key::Sqrt { (Op::Sqrt, v.sqrt()) } else { (Op::Recip, 1.0 / v) };
                self.push(v, op);
                self.set_result(r);
            }
            Key::TaxPlus | Key::TaxMinus => {
                let v = self.take();
                let factor = 1.0 + self.tax_rate / 100.0;
                let (op, r) =
                    if key == Key::TaxPlus { (Op::TaxPlus, v * factor) } else { (Op::TaxMinus, v / factor) };
                self.push(v, op);
                self.set_result(r);
            }
            Key::GrandTotal => {
                self.entry.clear();
                self.push(self.grand_total, Op::GrandTotal);
                self.set_result(self.grand_total);
            }
            Key::MemoryRecall => {
                self.entry.clear();
                self.value = self.memory;
            }
            Key::MemoryClear => self.memory = 0.0,
            Key::MemoryStore => self.memory = self.take(),
            Key::MemoryAdd => self.memory += self.take(),
            Key::MemorySub => self.memory -= self.take(),
        }
    }

    fn type_digits(&mut self, digits: &str) {
        let count = self.entry.chars().filter(char::is_ascii_digit).count();
        if count + digits.len() > MAX_DIGITS {
            return;
        }
        if self.entry == "0" {
            self.entry.clear();
        }
        self.entry.push_str(digits);
    }

    /// "=": ends the calculation and adds the result to the grand total.
    fn finish(&mut self, op: Op) {
        let v = self.take();
        let r = match self.pending.take() {
            Some(p) => {
                self.push(v, Op::Total);
                apply(p, self.acc, v)
            }
            None => v,
        };
        self.set_result(r);
        if !self.error {
            self.push(r, op);
            self.grand_total += r;
            self.acc = r;
        }
    }

    /// "Rückgängig": removes the last tape line.
    pub fn undo(&mut self) {
        if let Some(line) = self.tape.pop() {
            self.redo.push(line);
        }
    }

    pub fn redo(&mut self) {
        if let Some(line) = self.redo.pop() {
            self.tape.push(line);
        }
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear_tape(&mut self) {
        self.tape.clear();
        self.redo.clear();
        self.grand_total = 0.0;
    }

    /// The tape as text for the page: optional title, then one line per
    /// entry with the operation on the right.
    pub fn tape_text(&self, title: &str, decimal_comma: bool) -> String {
        let mut lines: Vec<String> = Vec::new();
        if !title.trim().is_empty() {
            lines.push(title.trim().to_string());
        }
        for l in &self.tape {
            let mut n = format_number(l.value, 2);
            if decimal_comma {
                n = n.replace('.', ",");
            }
            lines.push(format!("{n} {}", l.op.symbol()));
        }
        lines.join("\n")
    }
}

/// Fixed decimals, without "-0.00".
pub fn format_number(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
        s.trim_start_matches('-').into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(c: &mut Calculator, text: &str) {
        for ch in text.chars() {
            let k = match ch {
                '0'..='9' => Key::Digit(ch as u8 - b'0'),
                '.' => Key::Point,
                '+' => Key::Add,
                '-' => Key::Sub,
                '*' => Key::Mul,
                '/' => Key::Div,
                '=' => Key::Equals,
                '%' => Key::Percent,
                _ => continue,
            };
            c.press(k);
        }
    }

    #[test]
    fn chained_operations_and_tape() {
        let mut c = Calculator::default();
        keys(&mut c, "12.5+7.5*2=");
        assert_eq!(c.current(), 40.0);
        assert_eq!(c.display(true), "40,00");
        let ops: Vec<Op> = c.tape.iter().map(|l| l.op).collect();
        assert_eq!(ops, [Op::Add, Op::Mul, Op::Total, Op::Total]);
        assert_eq!(c.tape_text("Summe", false), "Summe\n12.50 +\n7.50 ×\n2.00 =\n40.00 =");
        assert_eq!(c.grand_total, 40.0);
    }

    #[test]
    fn percent_tax_memory_and_grand_total() {
        let mut c = Calculator::default();
        keys(&mut c, "200+10%");
        assert_eq!(c.current(), 220.0);
        c.press(Key::MemoryStore);
        keys(&mut c, "100");
        c.press(Key::TaxPlus);
        assert!((c.current() - 119.0).abs() < 1e-9);
        c.press(Key::TaxMinus);
        assert!((c.current() - 100.0).abs() < 1e-9);
        c.press(Key::MemoryRecall);
        assert_eq!(c.current(), 220.0);
        keys(&mut c, "=");
        c.press(Key::GrandTotal);
        assert_eq!(c.current(), 440.0);
    }

    #[test]
    fn errors_entry_editing_and_undo() {
        let mut c = Calculator::default();
        keys(&mut c, "5/0=");
        assert!(c.error);
        c.press(Key::Digit(3));
        assert_eq!(c.display(false), "E");
        c.press(Key::Clear);
        keys(&mut c, "1234");
        c.press(Key::Backspace);
        c.press(Key::Negate);
        assert_eq!(c.current(), -123.0);
        c.press(Key::DoubleZero);
        assert_eq!(c.current(), -12300.0);
        for _ in 0..20 {
            c.press(Key::Digit(9));
        }
        assert!(c.display(false).len() <= MAX_DIGITS + 2);
        let lines = c.tape.len();
        c.undo();
        assert_eq!(c.tape.len(), lines.saturating_sub(1));
        c.redo();
        assert_eq!(c.tape.len(), lines);
        assert_eq!(format_number(-0.001, 2), "0.00");
    }
}
