use crate::generator::ast::{BoolRef, BoolValue, U256Ref, U256Value};
use arbitrary::Unstructured;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationContext {
    input_words: u8,
    u256_locals: Vec<U256Ref>,
    bool_locals: Vec<BoolRef>,
}

impl GenerationContext {
    pub(crate) fn new(input_words: u8) -> Self {
        Self { input_words, u256_locals: Vec::new(), bool_locals: Vec::new() }
    }

    pub(crate) fn allocate_u256(&mut self) -> U256Ref {
        let local = U256Ref::new(self.u256_locals.len());
        self.u256_locals.push(local);
        local
    }

    pub(crate) fn allocate_bool(&mut self) -> BoolRef {
        let local = BoolRef::new(self.bool_locals.len());
        self.bool_locals.push(local);
        local
    }

    pub(crate) fn has_bool_values(&self) -> bool {
        !self.bool_locals.is_empty()
    }

    pub(crate) fn choose_u256_value(
        &self,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<U256Value> {
        let value_count = usize::from(self.input_words) + self.u256_locals.len();
        let index = u.int_in_range(0..=value_count - 1)?;

        if index < usize::from(self.input_words) {
            Ok(U256Value::Input(index as u8))
        } else {
            Ok(U256Value::Local(self.u256_locals[index - usize::from(self.input_words)]))
        }
    }

    pub(crate) fn choose_bool_value(
        &self,
        u: &mut Unstructured<'_>,
    ) -> arbitrary::Result<BoolValue> {
        debug_assert!(self.has_bool_values());

        let index = u.int_in_range(0..=self.bool_locals.len() - 1)?;
        Ok(BoolValue::Local(self.bool_locals[index]))
    }
}
