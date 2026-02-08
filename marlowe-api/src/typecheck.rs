use std::collections::{HashMap, HashSet};

use num_bigint::BigInt;

use crate::ast::{
    Action, Bound, Case, ChoiceId, Contract, DslType, Observation, Party, PayeeTarget, Timeout,
    Token, Value,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PartyRef {
    Role(String),
    Address(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenRef {
    pub currency_symbol: String,
    pub token_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChoiceRef {
    pub name: String,
    pub party: PartyRef,
}

#[derive(Debug, Clone, Default)]
pub struct TypeCheckContext {
    pub known_accounts: HashSet<PartyRef>,
    pub known_parties: HashSet<PartyRef>,
    pub known_tokens: HashSet<TokenRef>,
    pub known_choices: HashSet<ChoiceRef>,
    pub require_known_definitions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedSymbol {
    pub name: String,
    pub ty: DslType,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeCheckResult {
    pub ready_to_run: bool,
    pub holes: Vec<TypedSymbol>,
    pub params: Vec<TypedSymbol>,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

pub fn type_check(contract: &Contract, context: &TypeCheckContext) -> TypeCheckResult {
    let mut checker = Checker::new(context);
    checker.collect_declared_choices(contract);
    checker.check_contract(contract, "$", &mut Vec::new());
    checker.into_result()
}

struct SymbolRecord {
    ty: DslType,
    first_path: String,
}

struct LetRecord {
    path: String,
    used: bool,
}

struct Checker<'a> {
    context: &'a TypeCheckContext,
    holes: HashMap<String, SymbolRecord>,
    params: HashMap<String, SymbolRecord>,
    errors: Vec<Diagnostic>,
    warnings: Vec<Diagnostic>,
    declared_choices: HashSet<ChoiceRef>,
}

impl<'a> Checker<'a> {
    fn new(context: &'a TypeCheckContext) -> Self {
        Self {
            context,
            holes: HashMap::new(),
            params: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            declared_choices: HashSet::new(),
        }
    }

    fn into_result(self) -> TypeCheckResult {
        let mut holes: Vec<TypedSymbol> = self
            .holes
            .into_iter()
            .map(|(name, record)| TypedSymbol {
                name,
                ty: record.ty,
                path: record.first_path,
            })
            .collect();
        holes.sort_by(|a, b| a.name.cmp(&b.name));

        let mut params: Vec<TypedSymbol> = self
            .params
            .into_iter()
            .map(|(name, record)| TypedSymbol {
                name,
                ty: record.ty,
                path: record.first_path,
            })
            .collect();
        params.sort_by(|a, b| a.name.cmp(&b.name));

        let ready_to_run = self.errors.is_empty() && holes.is_empty() && params.is_empty();

        TypeCheckResult {
            ready_to_run,
            holes,
            params,
            errors: self.errors,
            warnings: self.warnings,
        }
    }

    fn collect_declared_choices(&mut self, contract: &Contract) {
        match contract {
            Contract::Hole(_) | Contract::Close => {}
            Contract::Pay { then, .. } => self.collect_declared_choices(then),
            Contract::If { then, else_, .. } => {
                self.collect_declared_choices(then);
                self.collect_declared_choices(else_);
            }
            Contract::When {
                cases,
                timeout_continuation,
                ..
            } => {
                for case in cases {
                    self.collect_declared_choices_from_case(case);
                }
                self.collect_declared_choices(timeout_continuation);
            }
            Contract::Let { then, .. } => self.collect_declared_choices(then),
            Contract::Assert { then, .. } => self.collect_declared_choices(then),
        }
    }

    fn collect_declared_choices_from_case(&mut self, case: &Case) {
        match case {
            Case::Hole(_) => {}
            Case::Case { action, then } => {
                if let Action::Choice { id, .. } = action {
                    if let Some(choice_ref) = self.choice_ref(id) {
                        self.declared_choices.insert(choice_ref);
                    }
                }
                self.collect_declared_choices(then);
            }
        }
    }

    fn check_contract(
        &mut self,
        contract: &Contract,
        path: &str,
        let_scopes: &mut Vec<HashMap<String, LetRecord>>,
    ) {
        match contract {
            Contract::Hole(name) => self.register_hole(name, DslType::Contract, path),
            Contract::Close => {}
            Contract::Pay {
                from,
                to,
                token,
                amount,
                then,
            } => {
                self.check_party(from, &field(path, "from"), PartyUsage::AccountOwner);
                self.check_payee(to, &field(path, "to"));
                self.check_token(token, &field(path, "token"));
                self.check_value(amount, &field(path, "amount"), let_scopes);
                self.check_contract(then, &field(path, "then"), let_scopes);
            }
            Contract::If { cond, then, else_ } => {
                self.check_observation(cond, &field(path, "cond"), let_scopes);
                self.check_contract(then, &field(path, "then"), let_scopes);
                self.check_contract(else_, &field(path, "else"), let_scopes);
            }
            Contract::When {
                cases,
                timeout,
                timeout_continuation,
            } => {
                for (idx, case) in cases.iter().enumerate() {
                    self.check_case(case, &index(&field(path, "cases"), idx), let_scopes);
                }
                self.check_timeout(timeout, &field(path, "timeout"));
                self.check_contract(
                    timeout_continuation,
                    &field(path, "timeout_continuation"),
                    let_scopes,
                );
            }
            Contract::Let { name, value, then } => {
                if self.is_let_name_defined(let_scopes, name) {
                    self.warn(
                        path,
                        format!("let binding '{name}' shadows an existing binding"),
                    );
                }
                self.check_value(value, &field(path, "value"), let_scopes);

                let mut frame = HashMap::new();
                frame.insert(
                    name.clone(),
                    LetRecord {
                        path: field(path, "name"),
                        used: false,
                    },
                );
                let_scopes.push(frame);
                self.check_contract(then, &field(path, "then"), let_scopes);
                let popped = let_scopes.pop().expect("frame pushed");
                for (let_name, record) in popped {
                    if !record.used {
                        self.warn(
                            &record.path,
                            format!("let binding '{let_name}' is never used"),
                        );
                    }
                }
            }
            Contract::Assert { cond, then } => {
                self.check_observation(cond, &field(path, "cond"), let_scopes);
                self.check_contract(then, &field(path, "then"), let_scopes);
            }
        }
    }

    fn check_case(
        &mut self,
        case: &Case,
        path: &str,
        let_scopes: &mut Vec<HashMap<String, LetRecord>>,
    ) {
        match case {
            Case::Hole(name) => self.register_hole(name, DslType::Case, path),
            Case::Case { action, then } => {
                self.check_action(action, &field(path, "action"), let_scopes);
                self.check_contract(then, &field(path, "then"), let_scopes);
            }
        }
    }

    fn check_action(
        &mut self,
        action: &Action,
        path: &str,
        let_scopes: &mut Vec<HashMap<String, LetRecord>>,
    ) {
        match action {
            Action::Hole(name) => self.register_hole(name, DslType::Action, path),
            Action::Deposit {
                into,
                by,
                token,
                amount,
            } => {
                self.check_party(into, &field(path, "into"), PartyUsage::AccountOwner);
                self.check_party(by, &field(path, "by"), PartyUsage::Actor);
                self.check_token(token, &field(path, "token"));
                self.check_value(amount, &field(path, "amount"), let_scopes);
            }
            Action::Choice { id, bounds } => {
                self.check_choice_id(id, &field(path, "id"));
                for (idx, bound) in bounds.iter().enumerate() {
                    self.check_bound(bound, &index(&field(path, "bounds"), idx), let_scopes);
                }
            }
            Action::Notify { if_ } => self.check_observation(if_, &field(path, "if"), let_scopes),
        }
    }

    fn check_bound(
        &mut self,
        bound: &Bound,
        path: &str,
        let_scopes: &mut Vec<HashMap<String, LetRecord>>,
    ) {
        match bound {
            Bound::Hole(name) => self.register_hole(name, DslType::Bound, path),
            Bound::Bound { from, to } => {
                self.check_value(from, &field(path, "from"), let_scopes);
                self.check_value(to, &field(path, "to"), let_scopes);

                match (self.static_value(from), self.static_value(to)) {
                    (Some(from_value), Some(to_value)) => {
                        if from_value > to_value {
                            self.error(path, "invalid bound range: 'from' is greater than 'to'");
                        }
                    }
                    _ => {
                        self.warn(
                            path,
                            "bound range cannot be statically validated until values are concrete"
                                .to_owned(),
                        );
                    }
                }
            }
        }
    }

    fn check_value(
        &mut self,
        value: &Value,
        path: &str,
        let_scopes: &mut Vec<HashMap<String, LetRecord>>,
    ) {
        match value {
            Value::Hole(name) => self.register_hole(name, DslType::Value, path),
            Value::Param(name) => self.register_param(name, DslType::Value, path),
            Value::Constant(_) | Value::TimeIntervalStart | Value::TimeIntervalEnd => {}
            Value::Add(a, b) | Value::Sub(a, b) | Value::Mul(a, b) | Value::Div(a, b) => {
                self.check_value(a, &index(path, 0), let_scopes);
                self.check_value(b, &index(path, 1), let_scopes);
            }
            Value::Neg(inner) | Value::Abs(inner) => {
                self.check_value(inner, &field(path, "inner"), let_scopes)
            }
            Value::ValueFromChoice(choice) => {
                self.check_choice_usage(choice, &field(path, "choice"))
            }
            Value::UseValue(name) => {
                if !self.mark_let_used(let_scopes, name) {
                    self.error(
                        path,
                        format!("UseValue references undefined let binding '{name}'"),
                    );
                }
            }
            Value::AvailableMoney { account, token } => {
                self.check_party(account, &field(path, "account"), PartyUsage::AccountOwner);
                self.check_token(token, &field(path, "token"));
            }
        }
    }

    fn check_observation(
        &mut self,
        obs: &Observation,
        path: &str,
        let_scopes: &mut Vec<HashMap<String, LetRecord>>,
    ) {
        match obs {
            Observation::Hole(name) => self.register_hole(name, DslType::Observation, path),
            Observation::True | Observation::False => {}
            Observation::And(a, b) | Observation::Or(a, b) => {
                self.check_observation(a, &index(path, 0), let_scopes);
                self.check_observation(b, &index(path, 1), let_scopes);
            }
            Observation::Not(inner) => {
                self.check_observation(inner, &field(path, "inner"), let_scopes)
            }
            Observation::ChoseSomething(choice) => {
                self.check_choice_usage(choice, &field(path, "choice"))
            }
            Observation::ValueGE(a, b)
            | Observation::ValueGT(a, b)
            | Observation::ValueLT(a, b)
            | Observation::ValueLE(a, b)
            | Observation::ValueEQ(a, b) => {
                self.check_value(a, &index(path, 0), let_scopes);
                self.check_value(b, &index(path, 1), let_scopes);
            }
        }
    }

    fn check_timeout(&mut self, timeout: &Timeout, path: &str) {
        match timeout {
            Timeout::Hole(name) => self.register_hole(name, DslType::Timeout, path),
            Timeout::Param(name) => self.register_param(name, DslType::Timeout, path),
            Timeout::PosixTime(_) => {}
        }
    }

    fn check_payee(&mut self, payee: &PayeeTarget, path: &str) {
        match payee {
            PayeeTarget::Hole(name) => self.register_hole(name, DslType::Payee, path),
            PayeeTarget::ToParty(party) => {
                self.check_party(party, &field(path, "party"), PartyUsage::Actor)
            }
            PayeeTarget::ToAccount(account) => {
                self.check_party(account, &field(path, "account"), PartyUsage::AccountOwner)
            }
        }
    }

    fn check_party(&mut self, party: &Party, path: &str, usage: PartyUsage) {
        match party {
            Party::Hole(name) => self.register_hole(name, DslType::Party, path),
            Party::Role(name) => {
                if self.context.require_known_definitions {
                    let party_ref = PartyRef::Role(name.clone());
                    if usage.requires_party() && !self.context.known_parties.contains(&party_ref) {
                        self.error(path, format!("unknown party role '{name}'"));
                    }
                    if usage.requires_account() && !self.context.known_accounts.contains(&party_ref)
                    {
                        self.error(path, format!("unknown account owner role '{name}'"));
                    }
                }
            }
            Party::Address(name) => {
                if self.context.require_known_definitions {
                    let party_ref = PartyRef::Address(name.clone());
                    if usage.requires_party() && !self.context.known_parties.contains(&party_ref) {
                        self.error(path, format!("unknown party address '{name}'"));
                    }
                    if usage.requires_account() && !self.context.known_accounts.contains(&party_ref)
                    {
                        self.error(path, format!("unknown account owner address '{name}'"));
                    }
                }
            }
        }
    }

    fn check_token(&mut self, token: &Token, path: &str) {
        match token {
            Token::Hole(name) => self.register_hole(name, DslType::Token, path),
            Token::Token {
                currency_symbol,
                token_name,
            } => {
                if self.context.require_known_definitions
                    && !self.context.known_tokens.contains(&TokenRef {
                        currency_symbol: currency_symbol.clone(),
                        token_name: token_name.clone(),
                    })
                {
                    self.error(
                        path,
                        format!("unknown token '{}.{}'", currency_symbol, token_name),
                    );
                }
            }
        }
    }

    fn check_choice_id(&mut self, choice_id: &ChoiceId, path: &str) {
        match choice_id {
            ChoiceId::Hole(name) => self.register_hole(name, DslType::ChoiceId, path),
            ChoiceId::ChoiceId { party, .. } => {
                self.check_party(party, &field(path, "party"), PartyUsage::Actor)
            }
        }
    }

    fn check_choice_usage(&mut self, choice_id: &ChoiceId, path: &str) {
        self.check_choice_id(choice_id, path);
        if let Some(choice_ref) = self.choice_ref(choice_id) {
            let known = self.declared_choices.contains(&choice_ref)
                || self.context.known_choices.contains(&choice_ref);
            if self.context.require_known_definitions && !known {
                self.error(
                    path,
                    format!(
                        "choice '{}' for party is not declared in contract or context",
                        choice_ref.name
                    ),
                );
            }
        }
    }

    fn choice_ref(&self, choice_id: &ChoiceId) -> Option<ChoiceRef> {
        match choice_id {
            ChoiceId::Hole(_) => None,
            ChoiceId::ChoiceId { name, party } => party_to_ref(party).map(|party| ChoiceRef {
                name: name.clone(),
                party,
            }),
        }
    }

    fn static_value(&self, value: &Value) -> Option<BigInt> {
        match value {
            Value::Constant(v) => Some(v.clone()),
            _ => None,
        }
    }

    fn register_hole(&mut self, name: &str, ty: DslType, path: &str) {
        self.register_symbol(true, name, ty, path);
    }

    fn register_param(&mut self, name: &str, ty: DslType, path: &str) {
        self.register_symbol(false, name, ty, path);
    }

    fn register_symbol(&mut self, is_hole: bool, name: &str, ty: DslType, path: &str) {
        let existing = {
            let symbols = if is_hole { &self.holes } else { &self.params };
            symbols
                .get(name)
                .map(|record| (record.ty, record.first_path.clone()))
        };

        if let Some((existing_ty, existing_path)) = existing {
            if existing_ty != ty {
                let kind = if is_hole { "hole" } else { "parameter" };
                self.error(
                    path,
                    format!(
                        "{kind} '{name}' has conflicting inferred types: {} at {} and {} at {path}",
                        existing_ty.as_str(),
                        existing_path,
                        ty.as_str()
                    ),
                );
            }
            return;
        }

        let symbols = if is_hole {
            &mut self.holes
        } else {
            &mut self.params
        };
        symbols.insert(
            name.to_owned(),
            SymbolRecord {
                ty,
                first_path: path.to_owned(),
            },
        );
    }

    fn is_let_name_defined(&self, let_scopes: &[HashMap<String, LetRecord>], name: &str) -> bool {
        let_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
    }

    fn mark_let_used(&self, let_scopes: &mut [HashMap<String, LetRecord>], name: &str) -> bool {
        for scope in let_scopes.iter_mut().rev() {
            if let Some(record) = scope.get_mut(name) {
                record.used = true;
                return true;
            }
        }
        false
    }

    fn error(&mut self, path: &str, message: impl Into<String>) {
        self.errors.push(Diagnostic {
            path: path.to_owned(),
            message: message.into(),
        });
    }

    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.warnings.push(Diagnostic {
            path: path.to_owned(),
            message: message.into(),
        });
    }
}

fn field(path: &str, key: &str) -> String {
    format!("{path}.{key}")
}

fn index(path: &str, idx: usize) -> String {
    format!("{path}[{idx}]")
}

fn party_to_ref(party: &Party) -> Option<PartyRef> {
    match party {
        Party::Role(name) => Some(PartyRef::Role(name.clone())),
        Party::Address(name) => Some(PartyRef::Address(name.clone())),
        Party::Hole(_) => None,
    }
}

#[derive(Clone, Copy)]
enum PartyUsage {
    Actor,
    AccountOwner,
}

impl PartyUsage {
    fn requires_party(self) -> bool {
        matches!(self, PartyUsage::Actor)
    }

    fn requires_account(self) -> bool {
        matches!(self, PartyUsage::AccountOwner)
    }
}
