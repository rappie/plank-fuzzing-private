use crate::generator::ast::{
    ArgValue, BoolRef, BoolValue, FunctionRef, FunctionSignature, ParamRef, U256Ref, U256Value,
    ValueType,
};
use arbitrary::Unstructured;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationContext {
    input_words: u8,
    params: Vec<ValueType>,
    u256_locals: Vec<U256Ref>,
    bool_locals: Vec<BoolRef>,
    mutable_u256_locals: Vec<U256Ref>,
    mutable_bool_locals: Vec<BoolRef>,
    functions: Vec<FunctionSignature>,
    next_u256: usize,
    next_bool: usize,
}

impl GenerationContext {
    pub(crate) fn for_init(input_words: u8, functions: Vec<FunctionSignature>) -> Self {
        Self {
            input_words,
            params: Vec::new(),
            u256_locals: Vec::new(),
            bool_locals: Vec::new(),
            mutable_u256_locals: Vec::new(),
            mutable_bool_locals: Vec::new(),
            functions,
            next_u256: 0,
            next_bool: 0,
        }
    }

    pub(crate) fn for_function(params: Vec<ValueType>, functions: Vec<FunctionSignature>) -> Self {
        Self {
            input_words: 0,
            params,
            u256_locals: Vec::new(),
            bool_locals: Vec::new(),
            mutable_u256_locals: Vec::new(),
            mutable_bool_locals: Vec::new(),
            functions,
            next_u256: 0,
            next_bool: 0,
        }
    }

    pub(crate) fn allocate_u256(&mut self, mutable: bool) -> U256Ref {
        let local = U256Ref::new(self.next_u256);
        self.next_u256 += 1;
        self.u256_locals.push(local);
        if mutable {
            self.mutable_u256_locals.push(local);
        }
        local
    }

    pub(crate) fn allocate_bool(&mut self, mutable: bool) -> BoolRef {
        let local = BoolRef::new(self.next_bool);
        self.next_bool += 1;
        self.bool_locals.push(local);
        if mutable {
            self.mutable_bool_locals.push(local);
        }
        local
    }

    pub(crate) fn fork(&self) -> Self {
        self.clone()
    }

    pub(crate) fn absorb_allocations(&mut self, child: &Self) {
        self.next_u256 = self.next_u256.max(child.next_u256);
        self.next_bool = self.next_bool.max(child.next_bool);
    }

    pub(crate) fn has_u256_values(&self) -> bool {
        self.input_words > 0
            || self.params.iter().any(|param| *param == ValueType::U256)
            || !self.u256_locals.is_empty()
    }

    pub(crate) fn has_bool_values(&self) -> bool {
        self.params.iter().any(|param| *param == ValueType::Bool) || !self.bool_locals.is_empty()
    }

    pub(crate) fn has_mutable_u256_values(&self) -> bool {
        !self.mutable_u256_locals.is_empty()
    }

    pub(crate) fn has_mutable_bool_values(&self) -> bool {
        !self.mutable_bool_locals.is_empty()
    }

    pub(crate) fn choose_u256_value(
        &self,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<U256Value> {
        debug_assert!(self.has_u256_values());

        let inputs = usize::from(self.input_words);
        let params = self.u256_params().count();
        let locals = self.u256_locals.len();
        let index = u.int_in_range(0..=inputs + params + locals - 1)?;

        if index < inputs {
            return Ok(U256Value::Input(index as u8));
        }

        let param_index = index - inputs;
        if param_index < params {
            let param =
                self.u256_params().nth(param_index).expect("u256 param index was counted above");
            return Ok(U256Value::Param(param));
        }

        Ok(U256Value::Local(self.u256_locals[param_index - params]))
    }

    pub(crate) fn choose_bool_value(
        &self,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<BoolValue> {
        debug_assert!(self.has_bool_values());

        let params = self.bool_params().count();
        let locals = self.bool_locals.len();
        let index = u.int_in_range(0..=params + locals - 1)?;

        if index < params {
            let param = self.bool_params().nth(index).expect("bool param index was counted above");
            return Ok(BoolValue::Param(param));
        }

        Ok(BoolValue::Local(self.bool_locals[index - params]))
    }

    pub(crate) fn choose_mutable_u256(
        &self,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<U256Ref> {
        debug_assert!(self.has_mutable_u256_values());

        let index = u.int_in_range(0..=self.mutable_u256_locals.len() - 1)?;
        Ok(self.mutable_u256_locals[index])
    }

    pub(crate) fn choose_mutable_bool(
        &self,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<BoolRef> {
        debug_assert!(self.has_mutable_bool_values());

        let index = u.int_in_range(0..=self.mutable_bool_locals.len() - 1)?;
        Ok(self.mutable_bool_locals[index])
    }

    pub(crate) fn callable_functions_returning(&self, return_type: ValueType) -> Vec<FunctionRef> {
        self.functions
            .iter()
            .filter(|function| {
                function.return_type == return_type
                    && function.params.iter().all(|param| self.has_value_for_type(*param))
            })
            .map(|function| function.id)
            .collect()
    }

    pub(crate) fn choose_function_returning(
        &self,
        return_type: ValueType,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<FunctionRef> {
        let functions = self.callable_functions_returning(return_type);
        debug_assert!(!functions.is_empty());

        let index = u.int_in_range(0..=functions.len() - 1)?;
        Ok(functions[index])
    }

    pub(crate) fn args_for_function(
        &self,
        function: FunctionRef,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<Vec<ArgValue>> {
        let signature = self
            .functions
            .iter()
            .find(|candidate| candidate.id == function)
            .expect("function was chosen from visible signatures");

        signature
            .params
            .iter()
            .map(|param| match param {
                ValueType::U256 => self.choose_u256_value(u).map(ArgValue::U256),
                ValueType::Bool => self.choose_bool_value(u).map(ArgValue::Bool),
            })
            .collect()
    }

    fn has_value_for_type(&self, value_type: ValueType) -> bool {
        match value_type {
            ValueType::U256 => self.has_u256_values(),
            ValueType::Bool => self.has_bool_values(),
        }
    }

    fn u256_params(&self) -> impl Iterator<Item = ParamRef> + '_ {
        self.params
            .iter()
            .enumerate()
            .filter(|(_, value_type)| **value_type == ValueType::U256)
            .map(|(index, _)| ParamRef::new(index))
    }

    fn bool_params(&self) -> impl Iterator<Item = ParamRef> + '_ {
        self.params
            .iter()
            .enumerate()
            .filter(|(_, value_type)| **value_type == ValueType::Bool)
            .map(|(index, _)| ParamRef::new(index))
    }
}
