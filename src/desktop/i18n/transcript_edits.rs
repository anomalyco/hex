//! Labs › Delete transcript lines: one-language strings for the lab card, the
//! selection bar, and its failure notice, kept beside the main language
//! tables.

/// French strings by their English source.
#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Delete transcript lines", "Supprimer des lignes de transcription"),
    ("Select lines in a saved meeting transcript and delete them. Summaries, copies, and the Markdown file use what's left", "Sélectionnez des lignes dans la transcription d’une réunion enregistrée et supprimez-les. Les résumés, les copies et le fichier Markdown utilisent ce qui reste"),
    ("Select lines", "Sélectionner des lignes"),
    ("{} lines selected", "{} lignes sélectionnées"),
    ("1 line selected", "1 ligne sélectionnée"),
    ("Delete lines…", "Supprimer les lignes…"),
    ("Delete {} lines", "Supprimer {} lignes"),
    ("Delete 1 line", "Supprimer 1 ligne"),
    ("Select the lines to delete.", "Sélectionnez les lignes à supprimer."),
    ("Its summary and chapters will be removed. Summarize again for a fresh one.", "Son résumé et ses chapitres seront supprimés. Relancez le résumé pour en créer de nouveaux."),
    ("Lines could not be deleted.", "Impossible de supprimer les lignes."),
];

/// Portuguese strings by their English source.
#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Delete transcript lines", "Excluir linhas da transcrição"),
    ("Select lines in a saved meeting transcript and delete them. Summaries, copies, and the Markdown file use what's left", "Selecione linhas na transcrição de uma reunião salva e exclua-as. Resumos, cópias e o arquivo Markdown usam o que restar"),
    ("Select lines", "Selecionar linhas"),
    ("{} lines selected", "{} linhas selecionadas"),
    ("1 line selected", "1 linha selecionada"),
    ("Delete lines…", "Excluir linhas…"),
    ("Delete {} lines", "Excluir {} linhas"),
    ("Delete 1 line", "Excluir 1 linha"),
    ("Select the lines to delete.", "Selecione as linhas que deseja excluir."),
    ("Its summary and chapters will be removed. Summarize again for a fresh one.", "O resumo e os capítulos serão removidos. Resuma novamente para criar novos."),
    ("Lines could not be deleted.", "Não foi possível excluir as linhas."),
];

/// Russian strings by their English source.
#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Delete transcript lines", "Удаление строк расшифровки"),
    ("Select lines in a saved meeting transcript and delete them. Summaries, copies, and the Markdown file use what's left", "Выбирайте строки в сохранённой расшифровке встречи и удаляйте их. Сводки, копии и файл Markdown используют оставшееся"),
    ("Select lines", "Выбрать строки"),
    ("{} lines selected", "Выбрано строк: {}"),
    ("1 line selected", "Выбрана 1 строка"),
    ("Delete lines…", "Удалить строки…"),
    ("Delete {} lines", "Удалить строк: {}"),
    ("Delete 1 line", "Удалить 1 строку"),
    ("Select the lines to delete.", "Выберите строки для удаления."),
    ("Its summary and chapters will be removed. Summarize again for a fresh one.", "Сводка и разделы будут удалены. Запустите сводку снова, чтобы создать новые."),
    ("Lines could not be deleted.", "Не удалось удалить строки."),
];
