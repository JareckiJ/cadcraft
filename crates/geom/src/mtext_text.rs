//! MTEXT inline-format helpers shared by the crates above the geometry layer.

/// Strip MTEXT formatting codes to plain text with `\n` paragraph breaks.
/// Stacked fractions `\S1/2;` become `1/2`; `\~` is a space; braces are dropped.
pub fn plain_mtext(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => {
                let code = chars.get(i + 1).copied().unwrap_or(' ');
                match code {
                    'P' => {
                        out.push('\n');
                        i += 2;
                    }
                    'N' => {
                        out.push('\n');
                        i += 2;
                    }
                    '~' => {
                        out.push(' ');
                        i += 2;
                    }
                    '\\' | '{' | '}' => {
                        out.push(code);
                        i += 2;
                    }
                    'S' => {
                        // \Snum^den; \Snum/den; \Snum#den;
                        let mut j = i + 2;
                        let mut stack = String::new();
                        while let Some(&ch) = chars.get(j) {
                            if ch == ';' {
                                break;
                            }
                            stack.push(match ch {
                                '^' | '#' => '/',
                                c => c,
                            });
                            j += 1;
                        }
                        // "6\S1/2;" reads as "6 1/2".
                        if out.chars().last().is_some_and(|c| c.is_ascii_digit()) && stack.starts_with(|c: char| c.is_ascii_digit()) {
                            out.push(' ');
                        }
                        out.push_str(stack.trim_end_matches('/'));
                        i = j + 1;
                    }
                    'L' | 'l' | 'O' | 'o' | 'K' | 'k' => i += 2,
                    // Codes with an argument terminated by ';' (\f, \F, \H, \W, \Q, \T, \A, \C, \c, \p).
                    _ => {
                        let mut j = i + 2;
                        while let Some(&ch) = chars.get(j) {
                            if ch == ';' {
                                break;
                            }
                            j += 1;
                        }
                        i = j + 1;
                    }
                }
            }
            '{' | '}' => i += 1,
            '\r' => i += 1,
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}
