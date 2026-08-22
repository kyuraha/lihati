#[derive(Clone)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub line: usize,
}

fn fence_char(t: &str) -> Option<(u8, usize)> {
    let indent = t.len() - t.trim_start().len();
    if indent > 3 {
        return None;
    }
    let t = t.trim_start();
    for ch in ['`', '~'] {
        let count = t.chars().take_while(|&c| c == ch).count();
        if count >= 3 {
            return Some((ch as u8, count));
        }
    }
    None
}

pub fn outline(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut inside_fence: Option<u8> = None;
    for (idx, raw) in text.lines().enumerate() {
        let t = raw.trim_end();
        match inside_fence {
            Some(fc) => {
                let trimmed = t.trim_start();
                let only = trimmed.chars().all(|c| c == fc as char);
                if only && !trimmed.is_empty() {
                    inside_fence = None;
                }
                continue;
            }
            None => {
                if let Some((fc, _)) = fence_char(t) {
                    inside_fence = Some(fc);
                    continue;
                }
            }
        }
        let trimmed = t.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) {
            let rest = &trimmed[hashes..];
            if rest.starts_with(' ') || rest.starts_with('\t') {
                let title = rest.trim().trim_end_matches('#').trim().to_string();
                out.push(Heading { level: hashes as u8, title, line: idx });
            }
        }
    }
    out
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn find_matches(hay: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() || hay.is_empty() {
        return Vec::new();
    }
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    let n = nb.len();
    let mut out = Vec::new();

    if nb.iter().all(u8::is_ascii) {
        let mut i = 0usize;
        while i + n <= hb.len() {
            if hay.is_char_boundary(i)
                && hb[i..i + n]
                    .iter()
                    .zip(nb)
                    .all(|(a, b)| a.eq_ignore_ascii_case(b))
            {
                out.push(i);
                i += n;
            } else {
                i += 1;
            }
        }
    } else {
        let folded_hay = hay.to_lowercase();
        let folded_needle = needle.to_lowercase();
        let step = folded_needle.len().max(1);
        if folded_needle.is_empty() {
            return out;
        }
        let mut start = 0usize;
        while let Some(pos) = folded_hay[start..].find(&folded_needle) {
            let abs = start + pos;
            start += pos + step;
            if hay.is_char_boundary(abs) && hay.is_char_boundary((abs + n).min(hay.len())) {
                out.push(abs);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_outline() {
        let md = "# One\n\ntext\n## Two\n### Three\n# Four";
        let h = outline(md);
        let levels: Vec<u8> = h.iter().map(|x| x.level).collect();
        assert_eq!(levels, vec![1, 2, 3, 1]);
        assert_eq!(h[1].title, "Two");
    }

    #[test]
    fn skips_code_fences() {
        let md = "```rust\n# not a heading\n~~~\n# also not\n```\n# Real";
        assert_eq!(outline(md).len(), 1);
        assert_eq!(outline(md)[0].line, 5);
    }

    #[test]
    fn ignores_hashes_without_space() {
        let md = "#tag\n##also\n# real";
        let h = outline(md);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].title, "real");
    }

    #[test]
    fn closing_fences() {
        let md = "~~~md\n# nope\n~~~\n# yes";
        assert_eq!(outline(md).len(), 1);
    }

    #[test]
    fn crlf_and_trailing_hashes() {
        let md = "# Head ##\r\n\r\nbody\r\n## Sub";
        let h = outline(md);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].title, "Head");
        assert_eq!(h[0].line, 0);
        assert_eq!(h[1].title, "Sub");
        assert_eq!(h[1].line, 3);
    }

    #[test]
    fn unicode_headings() {
        let md = "# Café ☕\n## 中文标题\n### 日本語";
        let h = outline(md);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].title, "Café ☕");
        assert_eq!(h[1].title, "中文标题");
        assert_eq!(h[2].title, "日本語");
    }

    #[test]
    fn empty_and_whitespace() {
        assert!(outline("").is_empty());
        assert!(outline("   \n\t\n").is_empty());
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn find_case_insensitive_ascii() {
        assert_eq!(find_matches("Hello hello HELLO", "hello"), vec![0, 6, 12]);
        assert!(find_matches("", "x").is_empty());
        assert!(find_matches("abc", "").is_empty());
        assert_eq!(find_matches("aBcAbC", "ABC"), vec![0, 3]);
    }

    #[test]
    fn find_unicode() {
        assert_eq!(find_matches("中文 test 中文", "中文"), vec![0, 12]);
        assert_eq!(find_matches("Café café", "café"), vec![0, 6]);
        assert_eq!(find_matches("héllo wörld", "WÖRLD"), vec![7]);
        assert_eq!(find_matches("abc", "xyz"), Vec::<usize>::new());
    }

    #[test]
    fn words() {
        assert_eq!(word_count("hello world foo"), 3);
    }
}
