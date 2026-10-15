//! A project's language: the words its drawings print (a stair's UP, a title block's SCALE, a
//! schedule's headings) and the names commands give what they make (levels, views, sheets, rooms).
//! English, or Brazilian Portuguese. Names already given stay as they are; the core's messages,
//! which agents read, stay English.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "pt")]
    Portuguese,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Portuguese];

    /// Its code in commands and files: en, pt.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Portuguese => "pt",
        }
    }

    /// Its name, in itself.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Portuguese => "Português (Brasil)",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Some(Language::English),
            "pt" | "pt-br" | "portuguese" | "português" => Some(Language::Portuguese),
            _ => None,
        }
    }

    /// `english`, a word or a name the core writes, in this language.
    pub fn word(self, english: &str) -> &str {
        match self {
            Language::English => english,
            Language::Portuguese => portuguese(english).unwrap_or(english),
        }
    }

    /// Which of `options` (English words) `name` is, in English or in this language.
    pub fn find(self, name: &str, options: &[&'static str]) -> Option<&'static str> {
        let name = name.trim();
        options.iter().copied().find(|o| o.eq_ignore_ascii_case(name) || self.word(o).to_lowercase() == name.to_lowercase())
    }
}

/// The core's words in Brazilian Portuguese.
pub fn portuguese(english: &str) -> Option<&'static str> {
    Some(match english {
        // Names commands give.
        "Project" => "Projeto",
        "Level" => "Nível",
        "Section" => "Corte",
        "Elevation" => "Elevação",
        "North" => "Norte",
        "South" => "Sul",
        "East" => "Leste",
        "West" => "Oeste",
        "Unnamed" => "Sem nome",
        "Room" => "Ambiente",
        "Room Schedule" => "Tabela de ambientes",
        "Door Schedule" => "Tabela de portas",
        "Window Schedule" => "Tabela de janelas",
        "Wall Schedule" => "Tabela de paredes",
        "Floor Schedule" => "Tabela de pisos",
        "Level Schedule" => "Tabela de níveis",
        // What drawings print.
        "UP" => "SOBE",
        "DN" => "DESCE",
        "NOT ENCLOSED" => "NÃO DELIMITADO",
        "SCALE" => "ESCALA",
        "SCALE AS INDICATED" => "ESCALA CONFORME INDICADO",
        // Schedule headings.
        "Number" => "Número",
        "Name" => "Nome",
        "Mark" => "Marca",
        "Type" => "Tipo",
        "Width" => "Largura",
        "Height" => "Altura",
        "Sill" => "Peitoril",
        "Wall" => "Parede",
        "Length" => "Comprimento",
        "Thickness" => "Espessura",
        "Area (m²)" => "Área (m²)",
        "Area (ft²)" => "Área (ft²)",
        "Perimeter (m)" => "Perímetro (m)",
        "Perimeter (ft)" => "Perímetro (ft)",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_come_in_the_projects_language() {
        let pt = Language::Portuguese;
        assert_eq!([pt.word("Level"), pt.word("UP"), pt.word("SCALE AS INDICATED")], ["Nível", "SOBE", "ESCALA CONFORME INDICADO"]);
        assert_eq!(Language::English.word("Level"), "Level");
        assert_eq!(pt.word("Id"), "Id", "a word with no translation stays");
        assert_eq!(pt.find("tabela de ambientes", &["Room Schedule", "Door Schedule"]), Some("Room Schedule"));
        assert_eq!(pt.find("Door Schedule", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        assert_eq!(
            [Language::from_code("pt-BR"), Language::from_code("en"), Language::from_code("de")],
            [Some(pt), Some(Language::English), None]
        );
    }
}
