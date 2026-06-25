use alloy_primitives::U256;

pub(crate) fn encode_calldata_words(words: &[u64]) -> Vec<u8> {
    let mut calldata = Vec::with_capacity(words.len() * 32);

    for &word in words {
        calldata.extend_from_slice(&U256::from(word).to_be_bytes::<32>());
    }

    calldata
}

#[cfg(test)]
mod tests {
    use super::encode_calldata_words;

    #[test]
    fn encoded_calldata_has_one_word_per_input() {
        let calldata = encode_calldata_words(&[1, 2, 3]);

        assert_eq!(calldata.len(), 96);
        assert_eq!(calldata[31], 1);
        assert_eq!(calldata[63], 2);
        assert_eq!(calldata[95], 3);
    }
}
