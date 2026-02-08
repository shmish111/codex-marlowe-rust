use num_bigint::BigInt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DslType {
    Contract,
    Case,
    Action,
    Value,
    Observation,
    Timeout,
    Party,
    Token,
    ChoiceId,
    Bound,
    Payee,
}

impl DslType {
    pub fn as_str(self) -> &'static str {
        match self {
            DslType::Contract => "Contract",
            DslType::Case => "Case",
            DslType::Action => "Action",
            DslType::Value => "Value",
            DslType::Observation => "Observation",
            DslType::Timeout => "Timeout",
            DslType::Party => "Party",
            DslType::Token => "Token",
            DslType::ChoiceId => "ChoiceId",
            DslType::Bound => "Bound",
            DslType::Payee => "Payee",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contract {
    Hole(String),
    Close,
    Pay {
        from: Party,
        to: PayeeTarget,
        token: Token,
        amount: Value,
        then: Box<Contract>,
    },
    If {
        cond: Observation,
        then: Box<Contract>,
        else_: Box<Contract>,
    },
    When {
        cases: Vec<Case>,
        timeout: Timeout,
        timeout_continuation: Box<Contract>,
    },
    Let {
        name: String,
        value: Value,
        then: Box<Contract>,
    },
    Assert {
        cond: Observation,
        then: Box<Contract>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Case {
    Hole(String),
    Case { action: Action, then: Box<Contract> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Hole(String),
    Deposit {
        into: Party,
        by: Party,
        token: Token,
        amount: Value,
    },
    Choice {
        id: ChoiceId,
        bounds: Vec<Bound>,
    },
    Notify {
        if_: Observation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Hole(String),
    Param(String),
    Constant(BigInt),
    Add(Box<Value>, Box<Value>),
    Sub(Box<Value>, Box<Value>),
    Mul(Box<Value>, Box<Value>),
    Div(Box<Value>, Box<Value>),
    Neg(Box<Value>),
    Abs(Box<Value>),
    ValueFromChoice(ChoiceId),
    UseValue(String),
    TimeIntervalStart,
    TimeIntervalEnd,
    AvailableMoney { account: Party, token: Token },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Hole(String),
    True,
    False,
    And(Box<Observation>, Box<Observation>),
    Or(Box<Observation>, Box<Observation>),
    Not(Box<Observation>),
    ChoseSomething(ChoiceId),
    ValueGE(Box<Value>, Box<Value>),
    ValueGT(Box<Value>, Box<Value>),
    ValueLT(Box<Value>, Box<Value>),
    ValueLE(Box<Value>, Box<Value>),
    ValueEQ(Box<Value>, Box<Value>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timeout {
    Hole(String),
    Param(String),
    PosixTime(BigInt),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Party {
    Hole(String),
    Role(String),
    Address(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    Hole(String),
    Token {
        currency_symbol: String,
        token_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceId {
    Hole(String),
    ChoiceId { name: String, party: Party },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Hole(String),
    Bound { from: Value, to: Value },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayeeTarget {
    Hole(String),
    ToParty(Party),
    ToAccount(Party),
}
