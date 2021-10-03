use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use ordered_float::OrderedFloat;
use pathfinding::astar;
use simple_error::SimpleError;

use crate::BoxResult;

const SYMBOLS_SPACE: &[u8; 32] = b"!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

pub fn compute_password_entropy(pwd: &str) -> BoxResult<(f64, Vec<String>)> {
    // load vocab file
    let word2rank = load_vocab("/home/samar/dev/cracken/vocab.txt")?;
    let raw_pwd = pwd.as_bytes();
    let amatch = astar(
        &0usize,
        |&n| {
            (n..=raw_pwd.len())
                .rev()
                .filter_map(|i| {
                    word2rank
                        .get(&raw_pwd[n..i])
                        .map(|rank| (i, OrderedFloat::<f64>((*rank as f64).log2())))
                })
                .collect::<Vec<_>>()
        },
        |_| OrderedFloat::<f64>(0f64),
        |&n| n == raw_pwd.len(),
    );
    let (best_path, entropy) =
        amatch.ok_or_else(|| SimpleError::new("bad characters in password"))?;

    let mut best_split = Vec::with_capacity(best_path.len() - 1);
    let mut prev = 0usize;
    for i in best_path.into_iter().skip(1) {
        let word_i = &raw_pwd[prev..i];
        best_split.push(String::from_utf8_lossy(word_i).to_string());
        prev = i;
    }
    Ok((entropy.into_inner(), best_split))
}

pub fn password_mask_cost(pwd: &str) -> f64 {
    pwd.bytes()
        .into_iter()
        .map(|ch| {
            if ch.is_ascii_digit() {
                10f64.log2()
            } else if ch.is_ascii_alphabetic() {
                26f64.log2()
            } else if SYMBOLS_SPACE.contains(&ch) {
                (SYMBOLS_SPACE.len() as f64).log2()
            } else {
                256f64.log2()
            }
        })
        .sum()
}

fn load_vocab(fname: &str) -> BoxResult<HashMap<Vec<u8>, usize>> {
    let file = File::open(fname)?;
    let mut reader = BufReader::new(file);
    let mut buffer: Vec<u8> = Vec::with_capacity(256);
    let mut word2rank: HashMap<Vec<u8>, usize> = HashMap::new();

    let mut rank = 1;

    loop {
        match reader.read_until(b'\n', &mut buffer)? {
            0 => break,
            _ => {
                if buffer.pop().is_some() {
                    let mut word = buffer.to_vec();
                    word.shrink_to_fit();
                    word2rank.insert(word, rank);
                    rank += 1;
                };
                buffer.clear();
            }
        }
    }

    let missing_rank = word2rank.len() + 1;
    for ch in 0..=255u8 {
        word2rank.entry(vec![ch]).or_insert(missing_rank);
    }

    word2rank.shrink_to_fit();
    Ok(word2rank)
}

#[cfg(test)]
mod tests {
    use crate::password_entropy;
    use crate::password_entropy::password_mask_cost;

    #[test]
    fn test_compute_password_entropy() {
        let pwd = "helloworld123!";
        let res = password_entropy::compute_password_entropy(pwd).unwrap();
        assert_eq!(
            res,
            (
                30.823060867312257,
                vec!["helloworld", "123", "!"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            ),
        );
    }

    #[test]
    fn test_compute_password_entropy_long_password() {
        let pwd = "helloworld123!helloworld123!helloworld123!";
        let res = password_entropy::compute_password_entropy(pwd).unwrap();
        assert_eq!(
            res,
            (
                92.46918260193678,
                vec![
                    "helloworld",
                    "123",
                    "!",
                    "helloworld",
                    "123",
                    "!",
                    "helloworld",
                    "123",
                    "!"
                ]
                .into_iter()
                .map(String::from)
                .collect()
            ),
        );
    }

    #[test]
    fn test_compute_password_entropy_random_password() {
        let pwd = "E93gtaaE6yF7xDOWv3ww2QE6qD-Wye4mk8O3Vaerem8";
        let res = password_entropy::compute_password_entropy(pwd).unwrap();
        assert_eq!(
            res,
            (
                206.14950164576396,
                vec![
                    "E", "9", "3", "g", "t", "a", "a", "E", "6", "y", "F", "7", "x", "DOW", "v",
                    "3", "w", "w", "2", "QE", "6", "q", "D-", "W", "y", "e", "4", "m", "k", "8",
                    "O", "3", "V", "a", "e", "r", "e", "m", "8"
                ]
                .into_iter()
                .map(String::from)
                .collect()
            ),
        );
    }

    #[test]
    fn test_password_mask_cost() {
        let cases: Vec<(&str, f64)> = vec![
            ("Aa123456!", 34.33244800560635),
            ("0123456789", 33.219280948873624),
            ("😃", 32.0),
            ("!@#$%^&*()", 50.0),
            (
                "E93gtaaE6yF7xDOWv3ww2QE6qD-Wye4mk8O3Vaerem8",
                187.25484030613498,
            ),
        ];
        for (pwd, expected_cost) in cases {
            assert_eq!(password_mask_cost(pwd), expected_cost);
        }
    }
}
