//! `formatUsageCurrency` (upstream `src/widgets/shared/currency.ts`):
//! `amount.toLocaleString('en-US', { style: 'currency', currency, … })`.

use crate::settings::NumberFormat;
use crate::widgets::shared::effective_decimals;

/// en-US currency symbols (ICU); other well-formed codes print as `CODE `.
fn symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "CAD" => "CA$",
        "AUD" => "A$",
        "BRL" => "R$",
        "CNY" => "CN¥",
        "INR" => "₹",
        "KRW" => "₩",
        "MXN" => "MX$",
        "NZD" => "NZ$",
        "HKD" => "HK$",
        "TWD" => "NT$",
        "ILS" => "₪",
        "VND" => "₫",
        "PHP" => "₱",
        "XCD" => "EC$",
        _ => return None,
    })
}

/// Prefixes followed by a no-break space, like a bare code.
fn spaced_symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "XAF" => "FCFA",
        "XOF" => "F\u{202f}CFA",
        "XPF" => "CFPF",
        _ => return None,
    })
}

/// Round |x| half away from zero at `max` fraction digits, on its shortest
/// decimal representation (ICU's behaviour: 1.005 -> 1.01), and drop trailing
/// zeros past `min`. Returns (integer digits, fraction digits).
fn decimal_digits(x: f64, min: usize, max: usize) -> (String, String) {
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mant.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').collect();
    // value = 0.d1d2d3… × 10^(exp+1)
    let point = exp + 1;
    let keep = point + max as i64;
    let mut int_frac: Vec<u8> = if keep <= 0 {
        // everything is below the rounding position
        let round_up = keep == 0 && digits.first().is_some_and(|d| *d >= 5);
        let mut v = vec![0u8; max + 1];
        if round_up {
            *v.last_mut().unwrap() = 1;
        }
        let int_len = 1usize;
        let (i, f) = v.split_at(int_len);
        return finish(i.to_vec(), f.to_vec(), min);
    } else {
        let mut v: Vec<u8> = digits.iter().take(keep as usize).copied().collect();
        while (v.len() as i64) < keep {
            v.push(0);
        }
        if digits.get(keep as usize).is_some_and(|d| *d >= 5) {
            let mut i = v.len();
            loop {
                if i == 0 {
                    v.insert(0, 1);
                    break;
                }
                i -= 1;
                if v[i] == 9 {
                    v[i] = 0;
                } else {
                    v[i] += 1;
                    break;
                }
            }
        }
        v
    };
    let int_len = int_frac.len() - max;
    let frac = int_frac.split_off(int_len);
    let int = if int_frac.is_empty() { vec![0] } else { int_frac };
    return finish(int, frac, min);

    fn finish(int: Vec<u8>, mut frac: Vec<u8>, min: usize) -> (String, String) {
        while frac.len() > min && frac.last() == Some(&0) {
            frac.pop();
        }
        let mut int: Vec<u8> = int;
        while int.len() > 1 && int[0] == 0 {
            int.remove(0);
        }
        let s = |v: &[u8]| v.iter().map(|d| (b'0' + d) as char).collect::<String>();
        (s(&int), s(&frac))
    }
}

fn group(int: &str) -> String {
    let b = int.as_bytes();
    let mut out = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

pub fn format_usage_currency(amount: f64, currency: Option<&str>, f: &NumberFormat) -> String {
    let fd = effective_decimals(f, 2);
    let min = if f.style.as_deref() == Some("compact") { 0 } else { fd };
    let code = match currency {
        Some(c) if c.len() == 3 && c.bytes().all(|b| b.is_ascii_alphabetic()) => c.to_ascii_uppercase(),
        Some(_) => "USD".to_string(),
        None => "USD".to_string(),
    };
    let (int, frac) = decimal_digits(amount, min, fd);
    let number = if frac.is_empty() { group(&int) } else { format!("{}.{frac}", group(&int)) };
    let sign = if amount < 0.0 || (amount == 0.0 && amount.is_sign_negative()) { "-" } else { "" };
    match symbol(&code) {
        Some(s) => format!("{sign}{s}{number}"),
        None => {
            let prefix = spaced_symbol(&code).map(String::from).unwrap_or(code);
            format!("{sign}{prefix}\u{a0}{number}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(v: f64, style: Option<&str>, decimals: Option<f64>) -> String {
        format_usage_currency(v, None, &NumberFormat { style: style.map(String::from), decimals })
    }

    #[test]
    fn matches_intl() {
        assert_eq!(usd(1234.5, None, None), "$1,234.50");
        assert_eq!(usd(1.005, None, None), "$1.01");
        assert_eq!(usd(0.125, None, None), "$0.13");
        assert_eq!(usd(194.7, Some("compact"), None), "$194.7");
        assert_eq!(usd(2.5, None, Some(0.0)), "$3");
        assert_eq!(usd(0.5, Some("whole"), None), "$1");
        assert_eq!(usd(-2.5, None, Some(0.0)), "-$3");
        assert_eq!(usd(-0.001, None, None), "-$0.00");
        assert_eq!(usd(1e-7, Some("compact"), None), "$0");
        assert_eq!(usd(123456789.125, None, None), "$123,456,789.13");
        assert_eq!(usd(1e21, None, None), "$1,000,000,000,000,000,000,000.00");
        assert_eq!(usd(1.0, None, Some(5.0)), "$1.00000");
        assert_eq!(usd(999.995, None, None), "$1,000.00");
        let eur = |c| format_usage_currency(1234.5, Some(c), &NumberFormat::default());
        assert_eq!(eur("eur"), "€1,234.50");
        assert_eq!(eur("CHF"), "CHF\u{a0}1,234.50");
        assert_eq!(eur("XOF"), "F\u{202f}CFA\u{a0}1,234.50");
        assert_eq!(eur("US"), "$1,234.50");
    }
}
