use std::fmt;

use num_bigint::BigInt;
use serde_yaml::{Mapping, Value as Yaml};

use crate::ast::{
    Action, Bound, Case, ChoiceId, Contract, Observation, Party, PayeeTarget, Timeout, Token, Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse_contract_yaml(input: &str) -> Result<Contract, ParseError> {
    let root: Yaml = serde_yaml::from_str(input).map_err(|err| ParseError {
        path: "$".to_owned(),
        message: format!("invalid yaml: {err}"),
    })?;

    parse_contract(&root, "$")
}

fn parse_contract(node: &Yaml, path: &str) -> Result<Contract, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Contract::Hole(name));
    }
    reject_param(node, path, "Contract")?;

    let (tag, body) = expect_variant(node, path, "Contract")?;
    match tag {
        "Close" => expect_empty(body, &field(path, "Close")).map(|_| Contract::Close),
        "Pay" => {
            let pay_path = field(path, "Pay");
            let map = expect_mapping(body, &pay_path, "Pay")?;
            let from = parse_party(required(map, &pay_path, "from")?, &field(&pay_path, "from"))?;
            let token = parse_token(
                required(map, &pay_path, "token")?,
                &field(&pay_path, "token"),
            )?;
            let amount = parse_value(
                required(map, &pay_path, "amount")?,
                &field(&pay_path, "amount"),
            )?;
            let then =
                parse_contract(required(map, &pay_path, "then")?, &field(&pay_path, "then"))?;

            let to_party = optional(map, "to_party")
                .map(|v| parse_party(v, &field(&pay_path, "to_party")))
                .transpose()?;
            let to_account = optional(map, "to_account")
                .map(|v| parse_party(v, &field(&pay_path, "to_account")))
                .transpose()?;

            let to = match (to_party, to_account) {
                (Some(party), None) => PayeeTarget::ToParty(party),
                (None, Some(account)) => PayeeTarget::ToAccount(account),
                (Some(_), Some(_)) => {
                    return Err(ParseError {
                        path: pay_path,
                        message: "Pay must include exactly one of 'to_party' or 'to_account'"
                            .to_owned(),
                    })
                }
                (None, None) => {
                    return Err(ParseError {
                        path: pay_path,
                        message: "Pay is missing one of 'to_party' or 'to_account'".to_owned(),
                    })
                }
            };

            Ok(Contract::Pay {
                from,
                to,
                token,
                amount,
                then: Box::new(then),
            })
        }
        "If" => {
            let if_path = field(path, "If");
            let map = expect_mapping(body, &if_path, "If")?;
            Ok(Contract::If {
                cond: parse_observation(
                    required(map, &if_path, "cond")?,
                    &field(&if_path, "cond"),
                )?,
                then: Box::new(parse_contract(
                    required(map, &if_path, "then")?,
                    &field(&if_path, "then"),
                )?),
                else_: Box::new(parse_contract(
                    required(map, &if_path, "else")?,
                    &field(&if_path, "else"),
                )?),
            })
        }
        "When" => {
            let when_path = field(path, "When");
            let map = expect_mapping(body, &when_path, "When")?;
            let cases_yaml = required(map, &when_path, "cases")?;
            let cases_seq = expect_sequence(cases_yaml, &field(&when_path, "cases"), "cases")?;
            let mut cases = Vec::with_capacity(cases_seq.len());
            for (idx, case_yaml) in cases_seq.iter().enumerate() {
                cases.push(parse_case(
                    case_yaml,
                    &index(&field(&when_path, "cases"), idx),
                )?);
            }

            Ok(Contract::When {
                cases,
                timeout: parse_timeout(
                    required(map, &when_path, "timeout")?,
                    &field(&when_path, "timeout"),
                )?,
                timeout_continuation: Box::new(parse_contract(
                    required(map, &when_path, "timeout_continuation")?,
                    &field(&when_path, "timeout_continuation"),
                )?),
            })
        }
        "Let" => {
            let let_path = field(path, "Let");
            let map = expect_mapping(body, &let_path, "Let")?;
            Ok(Contract::Let {
                name: parse_raw_string(
                    required(map, &let_path, "name")?,
                    &field(&let_path, "name"),
                )?,
                value: parse_value(
                    required(map, &let_path, "value")?,
                    &field(&let_path, "value"),
                )?,
                then: Box::new(parse_contract(
                    required(map, &let_path, "then")?,
                    &field(&let_path, "then"),
                )?),
            })
        }
        "Assert" => {
            let assert_path = field(path, "Assert");
            let map = expect_mapping(body, &assert_path, "Assert")?;
            Ok(Contract::Assert {
                cond: parse_observation(
                    required(map, &assert_path, "cond")?,
                    &field(&assert_path, "cond"),
                )?,
                then: Box::new(parse_contract(
                    required(map, &assert_path, "then")?,
                    &field(&assert_path, "then"),
                )?),
            })
        }
        other => Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Contract constructor '{other}'"),
        }),
    }
}

fn parse_case(node: &Yaml, path: &str) -> Result<Case, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Case::Hole(name));
    }
    reject_param(node, path, "Case")?;

    let (tag, body) = expect_variant(node, path, "Case")?;
    if tag != "Case" {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Case constructor '{tag}'"),
        });
    }

    let case_path = field(path, "Case");
    let map = expect_mapping(body, &case_path, "Case")?;
    Ok(Case::Case {
        action: parse_action(
            required(map, &case_path, "action")?,
            &field(&case_path, "action"),
        )?,
        then: Box::new(parse_contract(
            required(map, &case_path, "then")?,
            &field(&case_path, "then"),
        )?),
    })
}

fn parse_action(node: &Yaml, path: &str) -> Result<Action, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Action::Hole(name));
    }
    reject_param(node, path, "Action")?;

    let (tag, body) = expect_variant(node, path, "Action")?;
    match tag {
        "Deposit" => {
            let deposit_path = field(path, "Deposit");
            let map = expect_mapping(body, &deposit_path, "Deposit")?;
            Ok(Action::Deposit {
                into: parse_party(
                    required(map, &deposit_path, "into")?,
                    &field(&deposit_path, "into"),
                )?,
                by: parse_party(
                    required(map, &deposit_path, "by")?,
                    &field(&deposit_path, "by"),
                )?,
                token: parse_token(
                    required(map, &deposit_path, "token")?,
                    &field(&deposit_path, "token"),
                )?,
                amount: parse_value(
                    required(map, &deposit_path, "amount")?,
                    &field(&deposit_path, "amount"),
                )?,
            })
        }
        "Choice" => {
            let choice_path = field(path, "Choice");
            let map = expect_mapping(body, &choice_path, "Choice")?;
            let bounds_yaml = required(map, &choice_path, "bounds")?;
            let bounds_seq =
                expect_sequence(bounds_yaml, &field(&choice_path, "bounds"), "bounds")?;
            let mut bounds = Vec::with_capacity(bounds_seq.len());
            for (idx, bound_yaml) in bounds_seq.iter().enumerate() {
                bounds.push(parse_bound(
                    bound_yaml,
                    &index(&field(&choice_path, "bounds"), idx),
                )?);
            }
            Ok(Action::Choice {
                id: parse_choice_id(
                    required(map, &choice_path, "id")?,
                    &field(&choice_path, "id"),
                )?,
                bounds,
            })
        }
        "Notify" => {
            let notify_path = field(path, "Notify");
            let map = expect_mapping(body, &notify_path, "Notify")?;
            Ok(Action::Notify {
                if_: parse_observation(
                    required(map, &notify_path, "if")?,
                    &field(&notify_path, "if"),
                )?,
            })
        }
        other => Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Action constructor '{other}'"),
        }),
    }
}

fn parse_bound(node: &Yaml, path: &str) -> Result<Bound, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Bound::Hole(name));
    }
    reject_param(node, path, "Bound")?;

    let (tag, body) = expect_variant(node, path, "Bound")?;
    if tag != "Bound" {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Bound constructor '{tag}'"),
        });
    }
    let bound_path = field(path, "Bound");
    let map = expect_mapping(body, &bound_path, "Bound")?;
    Ok(Bound::Bound {
        from: parse_value(
            required(map, &bound_path, "from")?,
            &field(&bound_path, "from"),
        )?,
        to: parse_value(required(map, &bound_path, "to")?, &field(&bound_path, "to"))?,
    })
}

fn parse_choice_id(node: &Yaml, path: &str) -> Result<ChoiceId, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(ChoiceId::Hole(name));
    }
    reject_param(node, path, "ChoiceId")?;

    let (tag, body) = expect_variant(node, path, "ChoiceId")?;
    if tag != "ChoiceId" {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown ChoiceId constructor '{tag}'"),
        });
    }
    let choice_id_path = field(path, "ChoiceId");
    let map = expect_mapping(body, &choice_id_path, "ChoiceId")?;
    Ok(ChoiceId::ChoiceId {
        name: parse_raw_string(
            required(map, &choice_id_path, "name")?,
            &field(&choice_id_path, "name"),
        )?,
        party: parse_party(
            required(map, &choice_id_path, "party")?,
            &field(&choice_id_path, "party"),
        )?,
    })
}

fn parse_party(node: &Yaml, path: &str) -> Result<Party, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Party::Hole(name));
    }
    reject_param(node, path, "Party")?;

    let (tag, body) = expect_variant(node, path, "Party")?;
    match tag {
        "Role" => Ok(Party::Role(parse_raw_string(body, &field(path, "Role"))?)),
        "Address" => Ok(Party::Address(parse_raw_string(
            body,
            &field(path, "Address"),
        )?)),
        other => Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Party constructor '{other}'"),
        }),
    }
}

fn parse_token(node: &Yaml, path: &str) -> Result<Token, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Token::Hole(name));
    }
    reject_param(node, path, "Token")?;

    let (tag, body) = expect_variant(node, path, "Token")?;
    if tag != "Token" {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Token constructor '{tag}'"),
        });
    }

    let token_path = field(path, "Token");
    let map = expect_mapping(body, &token_path, "Token")?;
    Ok(Token::Token {
        currency_symbol: parse_raw_string(
            required(map, &token_path, "currency_symbol")?,
            &field(&token_path, "currency_symbol"),
        )?,
        token_name: parse_raw_string(
            required(map, &token_path, "token_name")?,
            &field(&token_path, "token_name"),
        )?,
    })
}

fn parse_observation(node: &Yaml, path: &str) -> Result<Observation, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Observation::Hole(name));
    }
    reject_param(node, path, "Observation")?;

    let (tag, body) = expect_variant(node, path, "Observation")?;
    match tag {
        "True" => expect_empty(body, &field(path, "True")).map(|_| Observation::True),
        "False" => expect_empty(body, &field(path, "False")).map(|_| Observation::False),
        "And" => {
            let and_path = field(path, "And");
            let pair = expect_pair(body, &and_path, "And")?;
            Ok(Observation::And(
                Box::new(parse_observation(&pair[0], &index(&and_path, 0))?),
                Box::new(parse_observation(&pair[1], &index(&and_path, 1))?),
            ))
        }
        "Or" => {
            let or_path = field(path, "Or");
            let pair = expect_pair(body, &or_path, "Or")?;
            Ok(Observation::Or(
                Box::new(parse_observation(&pair[0], &index(&or_path, 0))?),
                Box::new(parse_observation(&pair[1], &index(&or_path, 1))?),
            ))
        }
        "Not" => Ok(Observation::Not(Box::new(parse_observation(
            body,
            &field(path, "Not"),
        )?))),
        "ChoseSomething" => Ok(Observation::ChoseSomething(parse_choice_id(
            body,
            &field(path, "ChoseSomething"),
        )?)),
        "ValueGE" => {
            let ge_path = field(path, "ValueGE");
            let pair = expect_pair(body, &ge_path, "ValueGE")?;
            Ok(Observation::ValueGE(
                Box::new(parse_value(&pair[0], &index(&ge_path, 0))?),
                Box::new(parse_value(&pair[1], &index(&ge_path, 1))?),
            ))
        }
        "ValueGT" => {
            let gt_path = field(path, "ValueGT");
            let pair = expect_pair(body, &gt_path, "ValueGT")?;
            Ok(Observation::ValueGT(
                Box::new(parse_value(&pair[0], &index(&gt_path, 0))?),
                Box::new(parse_value(&pair[1], &index(&gt_path, 1))?),
            ))
        }
        "ValueLT" => {
            let lt_path = field(path, "ValueLT");
            let pair = expect_pair(body, &lt_path, "ValueLT")?;
            Ok(Observation::ValueLT(
                Box::new(parse_value(&pair[0], &index(&lt_path, 0))?),
                Box::new(parse_value(&pair[1], &index(&lt_path, 1))?),
            ))
        }
        "ValueLE" => {
            let le_path = field(path, "ValueLE");
            let pair = expect_pair(body, &le_path, "ValueLE")?;
            Ok(Observation::ValueLE(
                Box::new(parse_value(&pair[0], &index(&le_path, 0))?),
                Box::new(parse_value(&pair[1], &index(&le_path, 1))?),
            ))
        }
        "ValueEQ" => {
            let eq_path = field(path, "ValueEQ");
            let pair = expect_pair(body, &eq_path, "ValueEQ")?;
            Ok(Observation::ValueEQ(
                Box::new(parse_value(&pair[0], &index(&eq_path, 0))?),
                Box::new(parse_value(&pair[1], &index(&eq_path, 1))?),
            ))
        }
        other => Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Observation constructor '{other}'"),
        }),
    }
}

fn parse_value(node: &Yaml, path: &str) -> Result<Value, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Value::Hole(name));
    }
    if let Some(name) = parse_param(node) {
        return Ok(Value::Param(name));
    }

    let (tag, body) = expect_variant(node, path, "Value")?;
    match tag {
        "Constant" => Ok(Value::Constant(parse_integer(
            body,
            &field(path, "Constant"),
        )?)),
        "ConstantParam" => Ok(Value::Param(parse_raw_string(
            body,
            &field(path, "ConstantParam"),
        )?)),
        "Add" => {
            parse_value_pair("Add", body, path).map(|(a, b)| Value::Add(Box::new(a), Box::new(b)))
        }
        "Sub" => {
            parse_value_pair("Sub", body, path).map(|(a, b)| Value::Sub(Box::new(a), Box::new(b)))
        }
        "Mul" => {
            parse_value_pair("Mul", body, path).map(|(a, b)| Value::Mul(Box::new(a), Box::new(b)))
        }
        "Div" => {
            parse_value_pair("Div", body, path).map(|(a, b)| Value::Div(Box::new(a), Box::new(b)))
        }
        "Neg" => Ok(Value::Neg(Box::new(parse_value(
            body,
            &field(path, "Neg"),
        )?))),
        "Abs" => Ok(Value::Abs(Box::new(parse_value(
            body,
            &field(path, "Abs"),
        )?))),
        "ValueFromChoice" => Ok(Value::ValueFromChoice(parse_choice_id(
            body,
            &field(path, "ValueFromChoice"),
        )?)),
        "UseValue" => Ok(Value::UseValue(parse_raw_string(
            body,
            &field(path, "UseValue"),
        )?)),
        "TimeIntervalStart" => {
            expect_empty(body, &field(path, "TimeIntervalStart"))?;
            Ok(Value::TimeIntervalStart)
        }
        "TimeIntervalEnd" => {
            expect_empty(body, &field(path, "TimeIntervalEnd"))?;
            Ok(Value::TimeIntervalEnd)
        }
        "AvailableMoney" => {
            let avail_path = field(path, "AvailableMoney");
            let map = expect_mapping(body, &avail_path, "AvailableMoney")?;
            Ok(Value::AvailableMoney {
                account: parse_party(
                    required(map, &avail_path, "account")?,
                    &field(&avail_path, "account"),
                )?,
                token: parse_token(
                    required(map, &avail_path, "token")?,
                    &field(&avail_path, "token"),
                )?,
            })
        }
        other => Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Value constructor '{other}'"),
        }),
    }
}

fn parse_timeout(node: &Yaml, path: &str) -> Result<Timeout, ParseError> {
    if let Some(name) = parse_hole(node) {
        return Ok(Timeout::Hole(name));
    }
    if let Some(name) = parse_param(node) {
        return Ok(Timeout::Param(name));
    }

    let (tag, body) = expect_variant(node, path, "Timeout")?;
    match tag {
        "Timeout" => Ok(Timeout::PosixTime(parse_integer(
            body,
            &field(path, "Timeout"),
        )?)),
        "SlotParam" => Ok(Timeout::Param(parse_raw_string(
            body,
            &field(path, "SlotParam"),
        )?)),
        other => Err(ParseError {
            path: path.to_owned(),
            message: format!("unknown Timeout constructor '{other}'"),
        }),
    }
}

fn parse_value_pair(tag: &str, body: &Yaml, path: &str) -> Result<(Value, Value), ParseError> {
    let pair_path = field(path, tag);
    let pair = expect_pair(body, &pair_path, tag)?;
    Ok((
        parse_value(&pair[0], &index(&pair_path, 0))?,
        parse_value(&pair[1], &index(&pair_path, 1))?,
    ))
}

fn parse_integer(node: &Yaml, path: &str) -> Result<BigInt, ParseError> {
    match node {
        Yaml::Number(number) => {
            if let Some(v) = number.as_i64() {
                return Ok(BigInt::from(v));
            }
            if let Some(v) = number.as_u64() {
                return Ok(BigInt::from(v));
            }
            Err(ParseError {
                path: path.to_owned(),
                message: "integer out of supported yaml numeric range; pass as string".to_owned(),
            })
        }
        Yaml::String(raw) => {
            let value = unescape(raw);
            value.parse::<BigInt>().map_err(|_| ParseError {
                path: path.to_owned(),
                message: format!("invalid integer '{value}'"),
            })
        }
        _ => Err(ParseError {
            path: path.to_owned(),
            message: "expected integer".to_owned(),
        }),
    }
}

fn parse_raw_string(node: &Yaml, path: &str) -> Result<String, ParseError> {
    if let Yaml::String(raw) = node {
        Ok(unescape(raw))
    } else {
        Err(ParseError {
            path: path.to_owned(),
            message: "expected string".to_owned(),
        })
    }
}

fn parse_hole(node: &Yaml) -> Option<String> {
    if let Yaml::String(raw) = node {
        let value = unescape(raw);
        if is_prefixed_symbol(&value, '?') {
            return Some(value[1..].to_owned());
        }
    }
    None
}

fn parse_param(node: &Yaml) -> Option<String> {
    if let Yaml::String(raw) = node {
        let value = unescape(raw);
        if is_prefixed_symbol(&value, '$') {
            return Some(value[1..].to_owned());
        }
    }
    None
}

fn reject_param(node: &Yaml, path: &str, expected: &str) -> Result<(), ParseError> {
    if let Some(name) = parse_param(node) {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("parameter '${name}' is not valid in {expected} position"),
        });
    }
    Ok(())
}

fn is_prefixed_symbol(value: &str, prefix: char) -> bool {
    let mut chars = value.chars();
    if chars.next() != Some(prefix) {
        return false;
    }
    let rest: String = chars.collect();
    !rest.is_empty() && !rest.contains(' ')
}

fn unescape(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("??") {
        return format!("?{rest}");
    }
    if let Some(rest) = raw.strip_prefix("$$") {
        return format!("${rest}");
    }
    raw.to_owned()
}

fn expect_variant<'a>(
    node: &'a Yaml,
    path: &str,
    expected: &str,
) -> Result<(&'a str, &'a Yaml), ParseError> {
    let map = expect_mapping(node, path, expected)?;
    if map.len() != 1 {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("{expected} must be a single-key constructor mapping"),
        });
    }

    let (key, value) = map.iter().next().expect("map len checked");
    let key_string = key.as_str().ok_or_else(|| ParseError {
        path: path.to_owned(),
        message: format!("{expected} constructor key must be a string"),
    })?;
    Ok((key_string, value))
}

fn expect_mapping<'a>(
    node: &'a Yaml,
    path: &str,
    expected: &str,
) -> Result<&'a Mapping, ParseError> {
    if let Yaml::Mapping(map) = node {
        Ok(map)
    } else {
        Err(ParseError {
            path: path.to_owned(),
            message: format!("expected {expected} mapping"),
        })
    }
}

fn expect_sequence<'a>(
    node: &'a Yaml,
    path: &str,
    expected: &str,
) -> Result<&'a Vec<Yaml>, ParseError> {
    if let Yaml::Sequence(seq) = node {
        Ok(seq)
    } else {
        Err(ParseError {
            path: path.to_owned(),
            message: format!("expected {expected} sequence"),
        })
    }
}

fn expect_pair<'a>(
    node: &'a Yaml,
    path: &str,
    expected: &str,
) -> Result<&'a Vec<Yaml>, ParseError> {
    let seq = expect_sequence(node, path, expected)?;
    if seq.len() != 2 {
        return Err(ParseError {
            path: path.to_owned(),
            message: format!("{expected} requires exactly 2 elements"),
        });
    }
    Ok(seq)
}

fn expect_empty(node: &Yaml, path: &str) -> Result<(), ParseError> {
    match node {
        Yaml::Mapping(map) if map.is_empty() => Ok(()),
        _ => Err(ParseError {
            path: path.to_owned(),
            message: "expected empty mapping {}".to_owned(),
        }),
    }
}

fn required<'a>(map: &'a Mapping, path: &str, key: &str) -> Result<&'a Yaml, ParseError> {
    optional(map, key).ok_or_else(|| ParseError {
        path: path.to_owned(),
        message: format!("missing required key '{key}'"),
    })
}

fn optional<'a>(map: &'a Mapping, key: &str) -> Option<&'a Yaml> {
    map.iter().find_map(|(k, v)| match k {
        Yaml::String(name) if name == key => Some(v),
        _ => None,
    })
}

fn field(path: &str, key: &str) -> String {
    format!("{path}.{key}")
}

fn index(path: &str, idx: usize) -> String {
    format!("{path}[{idx}]")
}
