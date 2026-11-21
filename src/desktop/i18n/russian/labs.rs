//! The Labs and Vocabulary sheet Russian strings, split from
//! [`super::TRANSLATIONS`] to keep that table under the source cap.

#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, &str)] = &[
    ("Import…", "Импортировать…"),
    ("Export…", "Экспортировать…"),
    ("Import vocabulary", "Импортировать словарь"),
    ("Export vocabulary", "Экспортировать словарь"),
    ("CSV files", "Файлы CSV"),
    ("Text files", "Текстовые файлы"),
    ("Vocabulary files", "Файлы словаря"),
    ("Import terms from a CSV file in the Vocabulary sheet, or save the whole list as one", "Импортируйте термины из файла CSV в листе словаря или сохраните весь список как такой файл"),
    ("Delete meetings from the command line", "Удаление встреч из командной строки"),
    ("Delete a finished meeting and its files from a terminal with hex meetings delete", "Удалите завершённую встречу и её файлы из терминала с помощью hex meetings delete"),
];
