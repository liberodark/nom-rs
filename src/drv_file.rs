use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct ParsedDerivation {
    /// `output-name -> store-path` (e.g. "out", "dev").
    pub outputs: BTreeMap<String, String>,
    /// `drv-path -> [output-names]`.
    pub input_drvs: BTreeMap<String, Vec<String>>,
    /// store paths used as raw inputs.
    pub input_srcs: Vec<String>,
    pub platform: String,
    /// `env-var -> value`. We only read the `pname` key out of it.
    pub env: BTreeMap<String, String>,
}

/// Read and parse a derivation file from disk.
pub fn read(path: &Path) -> Result<ParsedDerivation, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let s = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    parse(s).ok_or_else(|| format!("could not parse derivation: {}", path.display()))
}

/// Parse the textual contents of a `.drv` file.
pub fn parse(input: &str) -> Option<ParsedDerivation> {
    let mut p = Parser::new(input);
    p.expect("Derive(")?;
    // 1: outputs list
    let outputs_raw = p.parse_list(|p| {
        p.expect("(")?;
        let name = p.parse_string()?;
        p.expect(",")?;
        let path = p.parse_string()?;
        p.expect(",")?;
        let _hash_algo = p.parse_string()?;
        p.expect(",")?;
        let _hash = p.parse_string()?;
        p.expect(")")?;
        Some((name, path))
    })?;
    p.expect(",")?;
    // 2: input drvs list
    let input_drvs_raw = p.parse_list(|p| {
        p.expect("(")?;
        let drv_path = p.parse_string()?;
        p.expect(",")?;
        let outs = p.parse_list(|p| p.parse_string())?;
        p.expect(")")?;
        Some((drv_path, outs))
    })?;
    p.expect(",")?;
    // 3: input sources list
    let input_srcs = p.parse_list(|p| p.parse_string())?;
    p.expect(",")?;
    // 4: platform string
    let platform = p.parse_string()?;
    p.expect(",")?;
    // 5: builder string (skip)
    let _builder = p.parse_string()?;
    p.expect(",")?;
    // 6: args list (skip)
    let _args = p.parse_list(|p| p.parse_string())?;
    p.expect(",")?;
    // 7: env list of (name, value)
    let env_raw = p.parse_list(|p| {
        p.expect("(")?;
        let n = p.parse_string()?;
        p.expect(",")?;
        let v = p.parse_string()?;
        p.expect(")")?;
        Some((n, v))
    })?;
    p.expect(")")?;
    Some(ParsedDerivation {
        outputs: outputs_raw.into_iter().collect(),
        input_drvs: input_drvs_raw.into_iter().collect(),
        input_srcs,
        platform,
        env: env_raw.into_iter().collect(),
    })
}

struct Parser<'a> {
    s: &'a str,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self { s }
    }

    fn expect(&mut self, lit: &str) -> Option<()> {
        let rest = self.s.strip_prefix(lit)?;
        self.s = rest;
        Some(())
    }

    fn peek(&self) -> Option<char> {
        self.s.chars().next()
    }

    fn parse_string(&mut self) -> Option<String> {
        let mut chars = self.s.char_indices();
        let (_, first) = chars.next()?;
        if first != '"' {
            return None;
        }
        let mut out = String::new();
        let byte_index;
        loop {
            let (i, c) = chars.next()?;
            let mut end_after = i + c.len_utf8();
            match c {
                '"' => {
                    byte_index = end_after;
                    break;
                }
                '\\' => {
                    let (i2, esc) = chars.next()?;
                    end_after = i2 + esc.len_utf8();
                    let _ = end_after;
                    match esc {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
        self.s = &self.s[byte_index..];
        Some(out)
    }

    fn parse_list<T>(&mut self, mut elem: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        self.expect("[")?;
        let mut out = Vec::new();
        if self.peek() == Some(']') {
            self.s = &self.s[1..];
            return Some(out);
        }
        loop {
            let v = elem(self)?;
            out.push(v);
            match self.peek()? {
                ',' => {
                    self.s = &self.s[1..];
                }
                ']' => {
                    self.s = &self.s[1..];
                    break;
                }
                _ => return None,
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_tiny_derivation() {
        let drv = r#"Derive([("out","/nix/store/aaa-foo","","")],[("/nix/store/bbb-bar.drv",["out"])],["/nix/store/ccc-src"],"x86_64-linux","/nix/store/ddd-sh",["-c","build"],[("pname","foo"),("version","1.0")])"#;
        let p = parse(drv).expect("parse");
        assert_eq!(p.outputs.get("out").unwrap(), "/nix/store/aaa-foo");
        assert_eq!(
            p.input_drvs.get("/nix/store/bbb-bar.drv").unwrap(),
            &vec!["out".to_string()]
        );
        assert_eq!(p.input_srcs, vec!["/nix/store/ccc-src".to_string()]);
        assert_eq!(p.platform, "x86_64-linux");
        assert_eq!(p.env.get("pname").unwrap(), "foo");
    }
}
