use num_bigint::BigInt;
use serde_yaml::{Mapping, Value as Yaml};

use crate::ast::{
    Action, Bound, Case, ChoiceId, Contract, Observation, Party, PayeeTarget, Timeout, Token, Value,
};

pub fn contract_to_yaml(contract: &Contract) -> Yaml {
    yaml_contract(contract)
}

pub fn contract_to_yaml_string(contract: &Contract) -> Result<String, serde_yaml::Error> {
    serde_yaml::to_string(&contract_to_yaml(contract))
}

fn yaml_contract(contract: &Contract) -> Yaml {
    match contract {
        Contract::Hole(name) => hole(name),
        Contract::Close => variant("Close", empty_map()),
        Contract::Pay {
            from,
            to,
            token,
            amount,
            then,
        } => {
            let mut body = Mapping::new();
            body.insert(key("from"), yaml_party(from));
            body.insert(key("token"), yaml_token(token));
            body.insert(key("amount"), yaml_value(amount));
            body.insert(key("then"), yaml_contract(then));
            match to {
                PayeeTarget::Hole(name) => {
                    body.insert(key("to_party"), hole(name));
                }
                PayeeTarget::ToParty(party) => {
                    body.insert(key("to_party"), yaml_party(party));
                }
                PayeeTarget::ToAccount(account) => {
                    body.insert(key("to_account"), yaml_party(account));
                }
            }
            variant("Pay", Yaml::Mapping(body))
        }
        Contract::If { cond, then, else_ } => {
            let mut body = Mapping::new();
            body.insert(key("cond"), yaml_observation(cond));
            body.insert(key("then"), yaml_contract(then));
            body.insert(key("else"), yaml_contract(else_));
            variant("If", Yaml::Mapping(body))
        }
        Contract::When {
            cases,
            timeout,
            timeout_continuation,
        } => {
            let mut body = Mapping::new();
            body.insert(
                key("cases"),
                Yaml::Sequence(cases.iter().map(yaml_case).collect()),
            );
            body.insert(key("timeout"), yaml_timeout(timeout));
            body.insert(
                key("timeout_continuation"),
                yaml_contract(timeout_continuation),
            );
            variant("When", Yaml::Mapping(body))
        }
        Contract::Let { name, value, then } => {
            let mut body = Mapping::new();
            body.insert(key("name"), Yaml::String(name.clone()));
            body.insert(key("value"), yaml_value(value));
            body.insert(key("then"), yaml_contract(then));
            variant("Let", Yaml::Mapping(body))
        }
        Contract::Assert { cond, then } => {
            let mut body = Mapping::new();
            body.insert(key("cond"), yaml_observation(cond));
            body.insert(key("then"), yaml_contract(then));
            variant("Assert", Yaml::Mapping(body))
        }
    }
}

fn yaml_case(case: &Case) -> Yaml {
    match case {
        Case::Hole(name) => hole(name),
        Case::Case { action, then } => {
            let mut body = Mapping::new();
            body.insert(key("action"), yaml_action(action));
            body.insert(key("then"), yaml_contract(then));
            variant("Case", Yaml::Mapping(body))
        }
    }
}

fn yaml_action(action: &Action) -> Yaml {
    match action {
        Action::Hole(name) => hole(name),
        Action::Deposit {
            into,
            by,
            token,
            amount,
        } => {
            let mut body = Mapping::new();
            body.insert(key("into"), yaml_party(into));
            body.insert(key("by"), yaml_party(by));
            body.insert(key("token"), yaml_token(token));
            body.insert(key("amount"), yaml_value(amount));
            variant("Deposit", Yaml::Mapping(body))
        }
        Action::Choice { id, bounds } => {
            let mut body = Mapping::new();
            body.insert(key("id"), yaml_choice_id(id));
            body.insert(
                key("bounds"),
                Yaml::Sequence(bounds.iter().map(yaml_bound).collect()),
            );
            variant("Choice", Yaml::Mapping(body))
        }
        Action::Notify { if_ } => {
            let mut body = Mapping::new();
            body.insert(key("if"), yaml_observation(if_));
            variant("Notify", Yaml::Mapping(body))
        }
    }
}

fn yaml_bound(bound: &Bound) -> Yaml {
    match bound {
        Bound::Hole(name) => hole(name),
        Bound::Bound { from, to } => {
            let mut body = Mapping::new();
            body.insert(key("from"), yaml_value(from));
            body.insert(key("to"), yaml_value(to));
            variant("Bound", Yaml::Mapping(body))
        }
    }
}

fn yaml_choice_id(choice_id: &ChoiceId) -> Yaml {
    match choice_id {
        ChoiceId::Hole(name) => hole(name),
        ChoiceId::ChoiceId { name, party } => {
            let mut body = Mapping::new();
            body.insert(key("name"), Yaml::String(name.clone()));
            body.insert(key("party"), yaml_party(party));
            variant("ChoiceId", Yaml::Mapping(body))
        }
    }
}

fn yaml_party(party: &Party) -> Yaml {
    match party {
        Party::Hole(name) => hole(name),
        Party::Role(name) => variant("Role", Yaml::String(name.clone())),
        Party::Address(address) => variant("Address", Yaml::String(address.clone())),
    }
}

fn yaml_token(token: &Token) -> Yaml {
    match token {
        Token::Hole(name) => hole(name),
        Token::Token {
            currency_symbol,
            token_name,
        } => {
            let mut body = Mapping::new();
            body.insert(
                key("currency_symbol"),
                Yaml::String(currency_symbol.clone()),
            );
            body.insert(key("token_name"), Yaml::String(token_name.clone()));
            variant("Token", Yaml::Mapping(body))
        }
    }
}

fn yaml_observation(observation: &Observation) -> Yaml {
    match observation {
        Observation::Hole(name) => hole(name),
        Observation::True => variant("True", empty_map()),
        Observation::False => variant("False", empty_map()),
        Observation::And(a, b) => variant("And", pair(yaml_observation(a), yaml_observation(b))),
        Observation::Or(a, b) => variant("Or", pair(yaml_observation(a), yaml_observation(b))),
        Observation::Not(inner) => variant("Not", yaml_observation(inner)),
        Observation::ChoseSomething(choice_id) => {
            variant("ChoseSomething", yaml_choice_id(choice_id))
        }
        Observation::ValueGE(a, b) => variant("ValueGE", pair(yaml_value(a), yaml_value(b))),
        Observation::ValueGT(a, b) => variant("ValueGT", pair(yaml_value(a), yaml_value(b))),
        Observation::ValueLT(a, b) => variant("ValueLT", pair(yaml_value(a), yaml_value(b))),
        Observation::ValueLE(a, b) => variant("ValueLE", pair(yaml_value(a), yaml_value(b))),
        Observation::ValueEQ(a, b) => variant("ValueEQ", pair(yaml_value(a), yaml_value(b))),
    }
}

fn yaml_value(value: &Value) -> Yaml {
    match value {
        Value::Hole(name) => hole(name),
        Value::Param(name) => variant("ConstantParam", Yaml::String(name.clone())),
        Value::Constant(v) => variant("Constant", integer(v)),
        Value::Add(a, b) => variant("Add", pair(yaml_value(a), yaml_value(b))),
        Value::Sub(a, b) => variant("Sub", pair(yaml_value(a), yaml_value(b))),
        Value::Mul(a, b) => variant("Mul", pair(yaml_value(a), yaml_value(b))),
        Value::Div(a, b) => variant("Div", pair(yaml_value(a), yaml_value(b))),
        Value::Neg(inner) => variant("Neg", yaml_value(inner)),
        Value::Abs(inner) => variant("Abs", yaml_value(inner)),
        Value::ValueFromChoice(choice_id) => variant("ValueFromChoice", yaml_choice_id(choice_id)),
        Value::UseValue(name) => variant("UseValue", Yaml::String(name.clone())),
        Value::TimeIntervalStart => variant("TimeIntervalStart", empty_map()),
        Value::TimeIntervalEnd => variant("TimeIntervalEnd", empty_map()),
        Value::AvailableMoney { account, token } => {
            let mut body = Mapping::new();
            body.insert(key("account"), yaml_party(account));
            body.insert(key("token"), yaml_token(token));
            variant("AvailableMoney", Yaml::Mapping(body))
        }
    }
}

fn yaml_timeout(timeout: &Timeout) -> Yaml {
    match timeout {
        Timeout::Hole(name) => hole(name),
        Timeout::Param(name) => variant("SlotParam", Yaml::String(name.clone())),
        Timeout::PosixTime(v) => variant("Timeout", integer(v)),
    }
}

fn empty_map() -> Yaml {
    Yaml::Mapping(Mapping::new())
}

fn hole(name: &str) -> Yaml {
    Yaml::String(format!("?{name}"))
}

fn key(value: &str) -> Yaml {
    Yaml::String(value.to_owned())
}

fn pair(a: Yaml, b: Yaml) -> Yaml {
    Yaml::Sequence(vec![a, b])
}

fn variant(tag: &str, body: Yaml) -> Yaml {
    let mut mapping = Mapping::new();
    mapping.insert(key(tag), body);
    Yaml::Mapping(mapping)
}

fn integer(value: &BigInt) -> Yaml {
    Yaml::String(value.to_string())
}
