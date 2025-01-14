use std::env;
use std::fs::File;
use std::io::{stdout, BufRead, BufReader, BufWriter, ErrorKind, Write};

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};

use crate::create_smartlist::{SmartlistBuilder, SmartlistTokenizer, DEFAULT_VOCAB_SIZE};
use crate::generators::get_word_generator;
use crate::helpers::RawFileReader;
use crate::password_entropy::EntropyEstimator;
use crate::{built_info, BoxResult};

const EXAMPLE_USAGE: &str = r#"
For specific subcommand help run: cracken <subcommand> --help


Example Usage:

  ## Generate Subcommand Examples:

  # all digits from 00000000 to 99999999
  cracken ?d?d?d?d?d?d?d?d

  # all digits from 0 to 99999999
  cracken -m 1 ?d?d?d?d?d?d?d?d

  # words with pwd prefix - pwd0000 to pwd9999
  cracken pwd?d?d?d?d

  # all passwords of length 8 starting with upper then 6 lowers then digit
  cracken ?u?l?l?l?l?l?l?d

  # same as above, write output to pwds.txt instead of stdout
  cracken -o pwds.txt ?u?l?l?l?l?l?l?d

  # custom charset - all hex values
  cracken -c 0123456789abcdef '?1?1?1?1'

  # 4 custom charsets - the order determines the id of the charset
  cracken -c 01 -c ab -c de -c ef '?1?2?3?4'

  # 4 lowercase chars with years 2000-2019 suffix
  cracken -c 01 '?l?l?l?l20?1?d'

  # starts with firstname from wordlist followed by 4 digits
  cracken -w firstnames.txt '?w1?d?d?d?d'

  # starts with firstname from wordlist with lastname from wordlist ending with symbol
  cracken -w firstnames.txt -w lastnames.txt -c '!@#$' '?w1?w2?1'

  # repeating wordlists multiple times and combining charsets
  cracken -w verbs.txt -w nouns.txt '?w1?w2?w1?w2?w2?d?d?d'


  ## Create Smartlists Subcommand Examples:

  # create smartlist from single file into smart.txt
  cracken create -f rockyou.txt --smartlist smart.txt

  # create smartlist from multiple files with multiple tokenization algorithms
  cracken create -t bpe -t unigram -t wordpiece -f rockyou.txt -f passwords.txt -f wikipedia.txt --smartlist smart.txt

  # create smartlist with minimum subword length of 3 and max numbers-only subwords of size 6
  cracken create -f rockyou.txt --min-word-len 3 --numbers-max-size 6 --smartlist smart.txt


  ## Entropy Subcommand Examples:

  # estimating entropy of a password
  cracken entropy --smartlist vocab.txt 'helloworld123!'

  # estimating entropy of a passwords file with a charset mask entropy (default is hybrid)
  cracken entropy --smartlist vocab.txt -t charset -p passwords.txt

  # estimating the entropy of a passwords file
  cracken entropy --smartlist vocab.txt -p passwords.txt
"#;

fn parse_args(args: Option<Vec<&str>>) -> ArgMatches<'static> {
    let osargs: Vec<String>;
    let mut args = match args {
        Some(itr) => itr,
        None => {
            osargs = env::args().collect();
            osargs.iter().map(|s| s.as_ref()).collect()
        }
    };

    // workaround for default subcommand
    if args.len() >= 2 && !vec!["generate", "entropy", "create", "--help"].contains(&args[1]) {
        args.insert(1, "generate");
    }

    App::new(format!(
        "Cracken v{} - {}",
        built_info::PKG_VERSION,
        built_info::PKG_DESCRIPTION
    )).setting(AppSettings::DisableHelpSubcommand)
        .setting(AppSettings::ArgRequiredElseHelp)
    .after_help(
        format!(
            "{}\n{}-v{} {}-{} compiler: {}\nmore info at: {}",
            EXAMPLE_USAGE,
            built_info::PKG_NAME,
            built_info::PKG_VERSION,
            built_info::CFG_OS,
            built_info::CFG_TARGET_ARCH,
            built_info::RUSTC_VERSION,
            built_info::PKG_HOMEPAGE,
        )
        .as_str())
        .subcommand(SubCommand::with_name("generate")
        .about("(default) - Generates newline separated words according to given mask and wordlist files")
        .display_order(0)
    .arg(
        Arg::with_name("mask")
            .long_help(
                r#"the wordlist mask to generate.
available masks are:
    builtin charsets:
    ?d - digits: "0123456789"
    ?l - lowercase: "abcdefghijklmnopqrstuvwxyz"
    ?u - uppercase: "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    ?s - symbols: " !\"\#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"
    ?a - all characters: ?d + ?l + ?u + ?s
    ?b - all binary values: (0-255)

    custom charsets ?1 to ?9:
    ?1 - first custom charset specified by --charset 'mychars'

    wordlists ?w1 to ?w9:
    ?w1 - first wordlist specified by --wordlist 'my-wordlist.txt'
"#,
            )
            .takes_value(true)
            .required_unless("masks-file"),
    )
    .arg(Arg::with_name("masks-file")
            .short("i")
            .long("masks-file")
            .help("a file containing masks to generate")
            .takes_value(true)
            .required_unless("mask"),
    )
    .arg(
        Arg::with_name("min-length")
            .short("m")
            .long("minlen")
            .help("minimum length of the mask to start from")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name("max-length")
            .short("x")
            .long("maxlen")
            .help("maximum length of the mask to start from")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name("stats")
            .short("s")
            .long("stats")
            .help("prints the number of words this command will generate and exits")
            .takes_value(false)
            .required(false),
    ).arg(
        Arg::with_name("custom-charset")
            .short("c")
            .long("custom-charset")
            .help("custom charset (string of chars). up to 9 custom charsets - ?1 to ?9. use ?1 on the mask for the first charset")
            .takes_value(true)
            .required(false)
            .multiple(true)
            .number_of_values(1)
            .max_values(9),
    )
    .arg(
        Arg::with_name("wordlist")
            .short("w")
            .long("wordlist")
            .help("filename containing newline (0xA) separated words. note: currently all wordlists loaded to memory")
            .takes_value(true)
            .required(false)
            .multiple(true)
            .number_of_values(1)
            .max_values(9),
    )
    .arg(
        Arg::with_name("output-file")
            .short("o")
            .long("output-file")
            .help("output file to write the wordlist to, defaults to stdout")
            .takes_value(true)
            .required(false),
    )).subcommand(SubCommand::with_name("entropy")
        .about(r#"
Computes the estimated entropy of password or password file.
The entropy of a password is the log2(len(keyspace)) of the password.

There are two types of keyspace size estimations:
  * mask - keyspace of each char (digit=10, lowercase=26...).
  * hybrid - finding minimal split into subwords and charsets.

"#)
        .arg(
        Arg::with_name("smartlist")
            .short("f")
            .long("smartlist")
            .help("smartlist input file to estimate entropy with, a newline separated text file")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .required(true),
        ).arg(
        Arg::with_name("password")
            .help("password to estimate entropy for")
            .takes_value(true)
            .required_unless("passwords-file")
        ).arg(
        Arg::with_name("passwords-file")
            .short("p")
            .long("passwords-file")
            .help("newline separated password file to estimate entropy for")
            .takes_value(true)
            .required(false)
            .conflicts_with("password"),
        ).arg(
        Arg::with_name("summary")
            .short("s")
            .long("summary")
            .help("output summary of entropy for password")
            .takes_value(false)
            .required(false)
            .conflicts_with("password"),
        ).arg(
        Arg::with_name("mask_type")
            .short("t")
            .long("mask-type")
            .help("type of mask to output, one of: charsets(charsets only), hybrid(charsets+wordlists)")
            .takes_value(true)
            .required(false)
            .possible_values(&["hybrid", "charset"])
            .conflicts_with("password"),
        )
    ).subcommand(SubCommand::with_name("create")
        .about("Create a new smartlist from input file(s)")
        .arg(
        Arg::with_name("file")
            .short("f")
            .long("file")
            .help("input filename, can be specified multiple times for multiple files")
            .takes_value(true)
            .required(true)
            .multiple(true)
            .number_of_values(1)
        )
        .arg(
            Arg::with_name("smartlist")
            .short("o")
            .long("smartlist")
            .help("output smartlist filename")
            .takes_value(true)
            .required(true)
        )
        .arg(
        Arg::with_name("tokenizer")
            .short("t")
            .long("tokenizer")
            .help("tokenizer to use, can be specified multiple times.\none of: bpe,unigram,wordpiece")
            .takes_value(true)
            .possible_values(&["bpe", "unigram", "wordpiece"])
            .required(false)
            .multiple(true)
            .number_of_values(1)
            .default_value("bpe")
        )
        .arg(
            Arg::with_name("quiet")
            .short("q")
            .long("quiet")
            .help("disables printing progress bar")
            .takes_value(false)
            .required(false)
        )
        .arg(
            Arg::with_name("vocab_max_size")
            .short("m")
            .long("vocab-max-size")
            .help("max vocabulary size")
            .takes_value(true)
            .required(false)
        )
        .arg(
            Arg::with_name("min_frequency")
            .long("min-frequency")
            .help("minimum frequency of a word, relevant only for BPE tokenizer")
            .takes_value(true)
            .required(false)
        )
        .arg(
            Arg::with_name("numbers_max_size")
            .long("numbers-max-size")
            .help("filters numbers (all digits) longer than the specified size")
            .takes_value(true)
            .required(false)
        )
        .arg(
            Arg::with_name("min_word_len")
            .long("min-word-len")
            .short("l")
            .help("filters words shorter than the specified length")
            .takes_value(true)
            .required(false)
        )
    )
    .get_matches_from(args)
}

/// helper for handling cast and optional values at same time, exiting on error
macro_rules! optional_value_t_or_exit {
    ($m:ident, $v:expr, $t:ty) => {
        optional_value_t_or_exit!($m.value_of($v), $t)
    };
    ($m:ident.value_of($v:expr), $t:ty) => {
        if let Some(v) = $m.value_of($v) {
            match v.parse::<$t>() {
                Ok(val) => Some(val),
                Err(_) => clap::Error::value_validation_auto(format!(
                    "The argument '{}' isn't a valid value",
                    v
                ))
                .exit(),
            }
        } else {
            None
        }
    };
}

pub fn run(args: Option<Vec<&str>>) -> BoxResult<()> {
    // parse args
    let arg_matches = parse_args(args);

    match arg_matches.subcommand() {
        ("generate", Some(matches)) => run_wordlist_generator(matches),
        ("create", Some(matches)) => run_create_smartlist(matches),
        ("entropy", Some(matches)) => run_entropy_estimator(matches),
        (_, None) => bail!("invalid command"),
        _ => unreachable!("oopsie, subcommand is required"),
    }
}

pub fn run_wordlist_generator(args: &ArgMatches) -> BoxResult<()> {
    let masks = match args.value_of("mask") {
        Some(mask) => vec![mask.to_owned()],
        None => {
            let masks_fname = args.value_of("masks-file").unwrap();
            let file = BufReader::new(File::open(masks_fname)?);
            let masks: Result<Vec<_>, _> = file.lines().collect();
            masks?
        }
    };

    let minlen = optional_value_t_or_exit!(args, "min-length", usize);
    let maxlen = optional_value_t_or_exit!(args, "max-length", usize);
    let outfile = args.value_of("output-file");

    // create output file
    let mut out: Box<dyn Write> = match outfile {
        Some(fname) => match File::create(fname) {
            Ok(fp) => Box::new(fp),
            Err(e) => bail!("cannot open file {}: {}", fname, e),
        },
        None => Box::new(stdout()),
    };

    let custom_charsets: Vec<&str> = args
        .values_of("custom-charset")
        .map(|x| x.collect())
        .unwrap_or_else(Vec::new);

    let wordlists: Vec<&str> = args
        .values_of("wordlist")
        .map(|x| x.collect())
        .unwrap_or_else(Vec::new);

    for mask in masks {
        // create output file
        let word_generator =
            get_word_generator(&mask, minlen, maxlen, &custom_charsets, &wordlists)?;
        if args.is_present("stats") {
            let combs = word_generator.combinations();
            println!("{}", combs);
            return Ok(());
        }

        match word_generator.gen(&mut out) {
            Ok(_) => {}
            Err(e) => {
                match e.kind() {
                    // ignore broken pipe, (e.g. happens when using head)
                    ErrorKind::BrokenPipe => return Ok(()),
                    _ => bail!("error occurred writing to out: {}", e),
                }
            }
        }
    }
    Ok(())
}

pub fn run_entropy_estimator(args: &ArgMatches) -> BoxResult<()> {
    let smartlist_files: Vec<&str> = args.values_of("smartlist").map(|x| x.collect()).unwrap();
    let est = EntropyEstimator::from_files(smartlist_files.as_ref())?;
    let is_summary_only = args.is_present("summary");
    let mask_type = args.value_of("mask_type").unwrap_or("hybrid");
    let mut total_entropy = 0f64;
    let mut pwd_count = 0usize;
    let mut stdout = stdout();

    if let Some(pwd) = args.value_of("password") {
        let entropy_result = est.estimate_password_entropy(pwd.as_bytes())?;
        let text = format!(
            "hybrid-min-split: {:?}
hybrid-mask: {}
hybrid-min-entropy: {:.2}
--
charset-mask: {}
charset-mask-entropy: {:.2}
            ",
            entropy_result.subword_entropy_min_split,
            entropy_result.min_subword_mask,
            entropy_result.subword_entropy,
            entropy_result.charset_mask,
            entropy_result.mask_entropy,
        );
        if let Err(e) = write!(&mut stdout, "{}", text) {
            match e.kind() {
                // ignore broken pipe, (e.g. happens when using head)
                ErrorKind::BrokenPipe => return Ok(()),
                _ => bail!("error occurred writing to out: {}", e),
            }
        }
    } else if let Some(pwd_file) = args.value_of("passwords-file") {
        let file = File::open(pwd_file)?;
        let reader = RawFileReader::new(file);
        for pwd in reader.into_iter() {
            let pwd = pwd?;
            let entropy_result = est.estimate_password_entropy(&pwd)?;
            let (pwd_entropy, pwd_mask) = match mask_type {
                "hybrid" => (
                    entropy_result.subword_entropy,
                    entropy_result.min_subword_mask,
                ),
                "charset" => (entropy_result.mask_entropy, entropy_result.charset_mask),
                _ => unreachable!("invalid entropy type"),
            };
            if !is_summary_only {
                if let Err(e) = writeln!(
                    &mut stdout,
                    "{:.2},{},{}",
                    pwd_entropy,
                    pwd_mask,
                    String::from_utf8_lossy(&pwd)
                ) {
                    match e.kind() {
                        // ignore broken pipe, (e.g. happens when using head)
                        ErrorKind::BrokenPipe => return Ok(()),
                        _ => bail!("error occurred writing to out: {}", e),
                    }
                }
            } else {
                total_entropy += pwd_entropy;
            }
            pwd_count += 1;
        }

        if is_summary_only {
            writeln!(
                &mut stdout,
                "avg entropy: {}",
                total_entropy / pwd_count as f64
            )?;
        }
    }
    Ok(())
}

pub fn run_create_smartlist(args: &ArgMatches) -> BoxResult<()> {
    let outfile = args.value_of("smartlist").unwrap();
    let infiles = args.values_of("file").map(|x| x.collect()).unwrap();
    let vocab_max_size =
        optional_value_t_or_exit!(args, "vocab_max_size", u32).unwrap_or(DEFAULT_VOCAB_SIZE);
    let min_frequency = optional_value_t_or_exit!(args, "min_frequency", u32).unwrap_or(0);
    let print_progress = !args.is_present("quiet");
    let numbers_max_size = optional_value_t_or_exit!(args, "numbers_max_size", u32);
    let min_word_len = optional_value_t_or_exit!(args, "min_word_len", u32).unwrap_or(1);

    let tokenizers = args
        .values_of("tokenizer")
        .map(|x| x.collect())
        .unwrap_or_else(|| vec!["unigram"])
        .into_iter()
        .map(|x| match x {
            "bpe" => SmartlistTokenizer::BPE,
            "unigram" => SmartlistTokenizer::Unigram,
            "wordpiece" => SmartlistTokenizer::WordPiece,
            _ => unreachable!("invalid tokenizer {}", x),
        });

    let mut writer = BufWriter::new(File::create(outfile)?);
    let vocab = SmartlistBuilder::new()
        .infiles(infiles)
        .min_frequency(min_frequency)
        .vocab_max_size(vocab_max_size)
        .tokenizers(tokenizers.into_iter())
        .print_progress(print_progress)
        .numbers_max_size(numbers_max_size)
        .min_word_len(min_word_len)
        .build()?;

    // write to file
    for word in vocab.iter() {
        writer.write_all(word.as_bytes())?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{runner, test_util};

    #[test]
    fn test_run_generate_smoke() {
        for args in vec![vec!["cracken", "generate", "?d"], vec!["cracken", "?d"]] {
            assert!(runner::run(Some(args)).is_ok());
        }
    }

    #[test]
    fn test_run_entropy_smoke() {
        let vocab_fname = test_util::wordlist_fname("vocab.txt");
        let args = Some(vec![
            "cracken",
            "entropy",
            "--smartlist",
            vocab_fname.to_str().unwrap(),
            "helloworld123!",
        ]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_dev_null() {
        let args = Some(vec!["cracken", "-o", "/dev/null", "?d"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_custom_charset() {
        let args = Some(vec!["cracken", "-c=abcdef0123456789", "?1"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_stats() {
        let args = Some(vec!["cracken", "-s", "?d?s?u?l?a?b"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_perm_denied() {
        let args = Some(vec!["cracken", "-o", "/tmp/this/dir/not/exisT", "?d"]);
        assert!(runner::run(args).is_err());
    }

    #[test]
    fn test_run_bad_args() {
        let args = Some(vec!["cracken", "-m", "2", "?d"]);
        assert!(runner::run(args).is_err());
    }

    #[test]
    fn test_run_bad_args2() {
        let args = Some(vec!["cracken", "?x"]);
        assert!(runner::run(args).is_err());
    }

    #[test]
    fn test_run_bad_args3() {
        let args = Some(vec!["cracken", "-x", "5", "?d"]);
        assert!(runner::run(args).is_err());
    }
}
