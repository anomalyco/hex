//! The Labs and Vocabulary sheet French strings, split from
//! [`super::TRANSLATIONS`] to keep that table under the source cap.

#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, &str)] = &[
    ("Import…", "Importer…"),
    ("Export…", "Exporter…"),
    ("Import vocabulary", "Importer le vocabulaire"),
    ("Export vocabulary", "Exporter le vocabulaire"),
    ("CSV files", "Fichiers CSV"),
    ("Text files", "Fichiers texte"),
    ("Vocabulary files", "Fichiers de vocabulaire"),
    ("Import terms from a CSV file in the Vocabulary sheet, or save the whole list as one", "Importez des termes depuis un fichier CSV dans la feuille Vocabulaire, ou enregistrez toute la liste dans un fichier"),
    ("Delete meetings from the command line", "Supprimer des réunions en ligne de commande"),
    ("Delete a finished meeting and its files from a terminal with hex meetings delete", "Supprimez une réunion terminée et ses fichiers depuis un terminal avec hex meetings delete"),
];
