//! JSONPath mini 求值器 — 移植自 server.py L756-836。
//! 语义对齐：点号/方括号、数组下标、数字字符串键转数值、
//! 不存在的路径返回 None（前端显示 —），永不报错。

/// 求值入口：`$.data.used` / `$['used']` / `$.items[0].percent`
pub fn eval(path: &str, obj: &serde_json::Value) -> Option<serde_json::Value> {
    let p = path.trim();
    if p.is_empty() || p == "$" {
        return Some(obj.clone());
    }
    let body = p.strip_prefix('$').unwrap_or(p);
    let tokens = tokenize(body);
    let mut cur = obj.clone();
    for tok in tokens {
        cur = index(cur, &tok)?;
    }
    Some(cur)
}

#[derive(Debug, PartialEq)]
enum Tok {
    Key(String),
    Idx(i64),
}

/// 把 `$.a.b[0]['c']` 的 body 拆成 token 序列
fn tokenize(body: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = body.trim();
    let mut chars = b.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                // 普通键：读到 . 或 [ 为止
                let mut k = String::new();
                while let Some(&n) = chars.peek() {
                    if n == '.' || n == '[' {
                        break;
                    }
                    k.push(n);
                    chars.next();
                }
                if !k.is_empty() {
                    out.push(Tok::Key(k));
                }
            }
            '[' => {
                // 方括号：'key' 或 123 或 "key"
                let mut raw = String::new();
                let mut closed = false;
                while let Some(n) = chars.next() {
                    if n == ']' {
                        closed = true;
                        break;
                    }
                    raw.push(n);
                }
                if !closed {
                    break;
                }
                let r = raw.trim();
                let r_unq = r
                    .strip_prefix('\'')
                    .and_then(|s| s.strip_suffix('\''))
                    .or_else(|| r.strip_prefix('"').and_then(|s| s.strip_suffix('"')));
                if let Some(k) = r_unq {
                    out.push(Tok::Key(k.to_string()));
                } else if let Ok(i) = r.parse::<i64>() {
                    out.push(Tok::Idx(i));
                } else {
                    // 非法内容：当 key 处理（Python 版 _jp_index 对 map 直接查字符串）
                    out.push(Tok::Key(r.to_string()));
                }
            }
            _ => {
                // 首个 token 前没有 .（如 "data.used" 不带 $）：读一个键
                let mut k = String::new();
                k.push(c);
                while let Some(&n) = chars.peek() {
                    if n == '.' || n == '[' {
                        break;
                    }
                    k.push(n);
                    chars.next();
                }
                if !k.is_empty() {
                    out.push(Tok::Key(k));
                }
            }
        }
    }
    out
}

/// 单步索引 — 对齐 _jp_index：
/// - object：先字符串键查，miss 时若键是纯数字字符串也试（不常见但 Python dict 行为如此）
/// - array：数字下标（支持负数）；字符串数字也转
fn index(cur: serde_json::Value, tok: &Tok) -> Option<serde_json::Value> {
    match tok {
        Tok::Key(k) => {
            if let Some(o) = cur.as_object() {
                if let Some(v) = o.get(k) {
                    return Some(v.clone());
                }
            }
            if let Some(a) = cur.as_array() {
                if let Ok(i) = k.parse::<i64>() {
                    return arr_get(a, i);
                }
            }
            None
        }
        Tok::Idx(i) => match &cur {
            serde_json::Value::Array(a) => arr_get(a, *i),
            serde_json::Value::Object(o) => o.get(&i.to_string()).cloned(),
            _ => None,
        },
    }
}

fn arr_get(a: &[serde_json::Value], i: i64) -> Option<serde_json::Value> {
    let len = a.len() as i64;
    let real = if i < 0 { len + i } else { i };
    if real >= 0 && real < len {
        Some(a[real as usize].clone())
    } else {
        None
    }
}

/// 数值化 — 对齐 _parse_numeric：
/// bool→1/0、数字原样、纯数字字符串转数字、其他 None
pub fn parse_numeric(val: &serde_json::Value) -> Option<f64> {
    match val {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        serde_json::Value::String(s) => {
            let t = s.trim();
            if let Ok(f) = t.parse::<f64>() {
                // 排除 "NaN"/"inf" 等 parse 成功但无意义的
                if t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+') {
                    Some(f)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(p: &str, o: serde_json::Value) -> Option<serde_json::Value> {
        eval(p, &o)
    }

    #[test]
    fn t01_root() {
        let o = json!({"a": 1});
        assert_eq!(ev("$", o.clone()), Some(o));
    }

    #[test]
    fn t02_simple_dot() {
        assert_eq!(ev("$.used", json!({"used": 12.5})), Some(json!(12.5)));
    }

    #[test]
    fn t03_nested() {
        assert_eq!(
            ev("$.data.usage.used", json!({"data": {"usage": {"used": 3}}})),
            Some(json!(3))
        );
    }

    #[test]
    fn t04_bracket_quoted() {
        assert_eq!(
            ev("$['used']", json!({"used": 7})),
            Some(json!(7))
        );
    }

    #[test]
    fn t05_array_index() {
        assert_eq!(
            ev("$.items[0].percent", json!({"items": [{"percent": 42}]})),
            Some(json!(42))
        );
    }

    #[test]
    fn t06_array_negative() {
        assert_eq!(
            ev("$.items[-1]", json!({"items": [1, 2, 3]})),
            Some(json!(3))
        );
    }

    #[test]
    fn t07_missing_path_none() {
        assert_eq!(ev("$.nope.deep", json!({"a": 1})), None);
    }

    #[test]
    fn t08_index_out_of_range() {
        assert_eq!(ev("$.items[5]", json!({"items": [1]})), None);
    }

    #[test]
    fn t09_numeric_string_key() {
        // 数字字符串键查数组（Python: "0" → 0）
        assert_eq!(ev("$.items['0']", json!({"items": [9, 8]})), Some(json!(9)));
    }

    #[test]
    fn t10_no_dollar_prefix() {
        // 不带 $ 也能工作（Python 版默认 root）
        assert_eq!(ev("data.used", json!({"data": {"used": 5}})), Some(json!(5)));
    }

    #[test]
    fn t11_rate_limit_primary_window() {
        // 真实用例：codex
        let o = json!({"rate_limit": {"primary_window": {"used_percent": 33.0}}});
        assert_eq!(
            ev("$.rate_limit.primary_window.used_percent", o),
            Some(json!(33.0))
        );
    }

    #[test]
    fn t12_mixed_brackets() {
        assert_eq!(
            ev("$['a'][0]['b']", json!({"a": [{"b": "x"}]})),
            Some(json!("x"))
        );
    }

    #[test]
    fn t13_null_value_is_some() {
        // JSON null 是有效值（区别于路径不存在）
        assert_eq!(ev("$.v", json!({"v": null})), Some(json!(null)));
    }

    #[test]
    fn t14_undefined_string_value() {
        // 数据里真实出现过的 "undefined" 字符串——原样返回不转义
        assert_eq!(
            ev("$.jsonpath_used", json!({"jsonpath_used": "undefined"})),
            Some(json!("undefined"))
        );
    }

    #[test]
    fn t15_empty_path_returns_root() {
        let o = json!([1, 2]);
        assert_eq!(ev("", o.clone()), Some(o));
    }

    #[test]
    fn t16_numeric_parsing() {
        assert_eq!(parse_numeric(&json!(42)), Some(42.0));
        assert_eq!(parse_numeric(&json!("12.5")), Some(12.5));
        assert_eq!(parse_numeric(&json!(true)), Some(1.0));
        assert_eq!(parse_numeric(&json!("abc")), None);
        assert_eq!(parse_numeric(&json!("42px")), None);
        assert_eq!(parse_numeric(&json!(null)), None);
    }

    #[test]
    fn t17_index_on_object_with_number_tok() {
        assert_eq!(
            ev("$.a[1]", json!({"a": {"1": "one"}})),
            Some(json!("one"))
        );
    }
}
