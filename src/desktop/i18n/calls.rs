//! Meetings › Settings › Calls' strings in the languages whose tables live
//! beside the main one: the answers a call gets and the Calls sheet.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Ask each time", "Demander à chaque fois"),
    ("Don't offer", "Ne pas proposer"),
    ("Always record", "Toujours enregistrer"),
    ("Calls", "Appels"),
    ("When a call starts", "Quand un appel commence"),
    ("Apps and websites", "Apps et sites web"),
    ("Apps and websites show up here after Hex sees a call in them.", "Les apps et sites web apparaissent ici dès que Hex y détecte un appel."),
    ("A small offer appears beside the menu bar", "Une petite proposition apparaît près de la barre des menus"),
    ("Recording starts with the call; you'll see it in the menu bar", "L’enregistrement démarre avec l’appel ; vous le verrez dans la barre des menus"),
    ("Start recordings yourself from the menu bar", "Lancez vous-même les enregistrements depuis la barre des menus"),
    ("A small offer appears on screen", "Une petite proposition apparaît à l’écran"),
    ("Recording starts with the call", "L’enregistrement démarre avec l’appel"),
    ("Start recordings yourself from Hex", "Lancez vous-même les enregistrements depuis Hex"),
    ("{} seconds after the call app leaves the microphone", "{} secondes après que l’app d’appel a libéré le micro"),
    ("{name}: {answer}", "{name} : {answer}"),
    ("{} apps and websites answer differently", "{} apps et sites web répondent autrement"),
    ("{} call ended", "Appel {} terminé"),
    ("Recording stops in {} s", "L'enregistrement s'arrête dans {} s"),
    ("Stop now", "Arrêter maintenant"),
    ("Couldn't record", "Impossible d'enregistrer"),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Ask each time", "Perguntar sempre"),
    ("Don't offer", "Não oferecer"),
    ("Always record", "Sempre gravar"),
    ("Calls", "Chamadas"),
    ("When a call starts", "Quando uma chamada começa"),
    ("Apps and websites", "Apps e sites"),
    ("Apps and websites show up here after Hex sees a call in them.", "Apps e sites aparecem aqui depois que o Hex detecta uma chamada neles."),
    ("A small offer appears beside the menu bar", "Uma pequena oferta aparece ao lado da barra de menus"),
    ("Recording starts with the call; you'll see it in the menu bar", "A gravação começa com a chamada; você a verá na barra de menus"),
    ("Start recordings yourself from the menu bar", "Inicie as gravações você mesmo pela barra de menus"),
    ("A small offer appears on screen", "Uma pequena oferta aparece na tela"),
    ("Recording starts with the call", "A gravação começa com a chamada"),
    ("Start recordings yourself from Hex", "Inicie as gravações você mesmo pelo Hex"),
    ("{} seconds after the call app leaves the microphone", "{} segundos depois que o app da chamada libera o microfone"),
    ("{name}: {answer}", "{name}: {answer}"),
    ("{} apps and websites answer differently", "{} apps e sites respondem de outra forma"),
    ("{} call ended", "Chamada do {} encerrada"),
    ("Recording stops in {} s", "A gravação para em {} s"),
    ("Stop now", "Parar agora"),
    ("Couldn't record", "Não foi possível gravar"),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Ask each time", "Спрашивать каждый раз"),
    ("Don't offer", "Не предлагать"),
    ("Always record", "Всегда записывать"),
    ("Calls", "Звонки"),
    ("When a call starts", "Когда начинается звонок"),
    ("Apps and websites", "Приложения и сайты"),
    ("Apps and websites show up here after Hex sees a call in them.", "Приложения и сайты появятся здесь, когда Hex заметит в них звонок."),
    ("A small offer appears beside the menu bar", "Рядом со строкой меню появляется небольшое предложение"),
    ("Recording starts with the call; you'll see it in the menu bar", "Запись начинается вместе со звонком; вы увидите её в строке меню"),
    ("Start recordings yourself from the menu bar", "Запускайте записи сами из строки меню"),
    ("A small offer appears on screen", "На экране появляется небольшое предложение"),
    ("Recording starts with the call", "Запись начинается вместе со звонком"),
    ("Start recordings yourself from Hex", "Запускайте записи сами в Hex"),
    ("{} seconds after the call app leaves the microphone", "Через {} с после того, как приложение звонка освободит микрофон"),
    ("{name}: {answer}", "{name}: {answer}"),
    ("{} apps and websites answer differently", "Другие ответы: {}"),
    ("{} call ended", "Звонок в {} завершён"),
    ("Recording stops in {} s", "Запись остановится через {} с"),
    ("Stop now", "Остановить сейчас"),
    ("Couldn't record", "Не удалось записать"),
];

#[rustfmt::skip]
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Ask each time", "Kérdezz minden alkalommal"),
    ("Don't offer", "Ne ajánld fel"),
    ("Always record", "Mindig vegyél fel"),
    ("Calls", "Hívások"),
    ("When a call starts", "Amikor hívás indul"),
    ("Apps and websites", "Alkalmazások és webhelyek"),
    ("Apps and websites show up here after Hex sees a call in them.", "Az alkalmazások és webhelyek itt jelennek meg, miután a Hex hívást lát bennük."),
    ("A small offer appears beside the menu bar", "Egy kis ajánlat jelenik meg a menüsor mellett"),
    ("Recording starts with the call; you'll see it in the menu bar", "A felvétel a hívással indul; a menüsorban látni fogod"),
    ("Start recordings yourself from the menu bar", "A felvételeket magad indítod a menüsorból"),
    ("A small offer appears on screen", "Egy kis ajánlat jelenik meg a képernyőn"),
    ("Recording starts with the call", "A felvétel a hívással indul"),
    ("Start recordings yourself from Hex", "A felvételeket magad indítod a Hexből"),
    ("{} seconds after the call app leaves the microphone", "{} másodperccel azután, hogy a hívás alkalmazása elengedi a mikrofont"),
    ("{name}: {answer}", "{name}: {answer}"),
    ("{} apps and websites answer differently", "{} alkalmazás és webhely válaszol másképp"),
    ("{} call ended", "{} hívás véget ért"),
    ("Recording stops in {} s", "A felvétel {} mp múlva leáll"),
    ("Stop now", "Leállítás most"),
    ("Couldn't record", "Nem sikerült felvenni"),
];
