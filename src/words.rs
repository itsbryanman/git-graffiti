pub fn translate(word: &str) -> Option<String> {
    word.chars()
        .map(|character| match character.to_ascii_lowercase() {
            'a'..='f' | '0'..='9' => Some(character.to_ascii_lowercase()),
            'o' => Some('0'),
            'i' | 'l' => Some('1'),
            'z' => Some('2'),
            's' => Some('5'),
            't' => Some('7'),
            'g' => Some('9'),
            _ => None,
        })
        .collect()
}

pub fn lines(phrase: &str) -> Vec<String> {
    let words: Vec<_> = phrase.split_whitespace().collect();
    let width = words.iter().map(|word| word.len()).max().unwrap_or(0);
    words
        .into_iter()
        .map(|word| match translate(word) {
            Some(hex) => format!("{word:<width$} -> {hex}"),
            None => format!("{word} can't be done"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_leetspeak_and_keeps_hex() {
        assert_eq!(translate("bad"), Some("bad".to_owned()));
        assert_eq!(translate("coffee"), Some("c0ffee".to_owned()));
        assert_eq!(translate("gist"), Some("9157".to_owned()));
        assert_eq!(translate("glitz"), Some("91172".to_owned()));
    }

    #[test]
    fn rejects_letters_with_no_hex_stand_in() {
        assert_eq!(translate("rust"), None);
        assert_eq!(lines("bad rust"), ["bad  -> bad", "rust can't be done"]);
    }
}
