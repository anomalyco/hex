//! The Annotate pane's Agents section and its lab, kept in their own table
//! so the main, French, Portuguese, and Russian tables stay under the source
//! cap.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Agents", ["Agenci", "助手", "エージェント", "Agenten", "Agentes", "Агенти"]),
    ("Let coding agents read your captures with the hex-annotate skill", ["Pozwól agentom kodującym czytać Twoje zrzuty dzięki umiejętności hex-annotate", "让编码助手用 hex-annotate 技能读取你的截图与录屏", "コーディングエージェントが hex-annotate スキルでキャプチャを読めるようにします", "Lass Coding-Agenten deine Aufnahmen mit dem hex-annotate-Skill lesen", "Deja que los agentes de código lean tus capturas con la habilidad hex-annotate", "Дозвольте агентам для коду читати ваші знімки й записи за допомогою навички hex-annotate"]),
    ("Not found", ["Nie znaleziono", "未找到", "見つかりません", "Nicht gefunden", "No encontrado", "Не знайдено"]),
    ("Installed", ["Zainstalowano", "已安装", "インストール済み", "Installiert", "Instalado", "Встановлено"]),
    ("Update available", ["Dostępna aktualizacja", "有可用更新", "更新があります", "Aktualisierung verfügbar", "Actualización disponible", "Доступне оновлення"]),
    ("Managed elsewhere", ["Zarządzane gdzie indziej", "在其他位置管理", "別の場所で管理", "Anderswo verwaltet", "Gestionado en otro lugar", "Керується в іншому місці"]),
    ("Install with the skills CLI", ["Zainstaluj przez CLI skills", "用 skills 命令行安装", "skills CLI でインストール", "Mit der skills-CLI installieren", "Instalar con la CLI skills", "Встановити через CLI skills"]),
    ("For skills you manage yourself", ["Dla umiejętności zarządzanych samodzielnie", "适合自行管理技能的情况", "スキルを自分で管理する場合", "Für Skills, die du selbst verwaltest", "Para habilidades que gestionas tú", "Для навичок, якими керуєте ви"]),
    ("Command-line tool", ["Narzędzie wiersza poleceń", "命令行工具", "コマンドラインツール", "Befehlszeilenwerkzeug", "Herramienta de línea de comandos", "Інструмент командного рядка"]),
    ("Installed at {}", ["Zainstalowano w {}", "已安装到 {}", "{} にインストール済み", "Installiert unter {}", "Instalado en {}", "Встановлено в {}"]),
    ("Owned by another program", ["Należy do innego programu", "属于另一个程序", "別のプログラムが所有", "Gehört zu einem anderen Programm", "Pertenece a otro programa", "Належить іншій програмі"]),
    ("{} isn't on your PATH", ["{} nie ma w PATH", "{} 不在你的 PATH 中", "{} が PATH にありません", "{} ist nicht in deinem PATH", "{} no está en tu PATH", "{} немає у вашому PATH"]),
    ("Move Hex to Applications first", ["Najpierw przenieś Hex do Aplikacji", "请先把 Hex 移到“应用程序”", "先に Hex をアプリケーションに移動してください", "Verschiebe Hex zuerst in den Ordner „Programme“", "Mueve Hex a Aplicaciones primero", "Спершу перемістіть Hex до «Програм»"]),
    ("The skill's zoom, inspect, and frames commands need Labs › Agent tools for captures on", ["Polecenia zoom, inspect i frames tej umiejętności wymagają włączenia Laboratorium › Narzędzia agenta do zrzutów", "该技能的 zoom、inspect 和 frames 命令需要开启实验室 › 截图的助手工具", "このスキルの zoom、inspect、frames コマンドには、ラボ › キャプチャのエージェントツール をオンにする必要があります", "Die Befehle zoom, inspect und frames dieses Skills brauchen Labs › Agenten-Werkzeuge für Aufnahmen", "Los comandos zoom, inspect y frames de la habilidad necesitan Labs › Herramientas de agente para capturas", "Команди zoom, inspect і frames цієї навички потребують увімкненої Лабораторії › Інструменти агента для знімків"]),
    ("Couldn't install the skill: {}", ["Nie udało się zainstalować umiejętności: {}", "无法安装技能：{}", "スキルをインストールできませんでした: {}", "Der Skill konnte nicht installiert werden: {}", "No se pudo instalar la habilidad: {}", "Не вдалося встановити навичку: {}"]),
    ("Couldn't link the command-line tool: {}", ["Nie udało się dodać narzędzia wiersza poleceń: {}", "无法链接命令行工具：{}", "コマンドラインツールをリンクできませんでした: {}", "Das Befehlszeilenwerkzeug konnte nicht verlinkt werden: {}", "No se pudo enlazar la herramienta de línea de comandos: {}", "Не вдалося додати інструмент командного рядка: {}"]),
    ("Install agent skill from Annotate", ["Zainstaluj umiejętność agenta z Annotate", "从 Annotate 安装助手技能", "Annotate からエージェントスキルをインストール", "Agenten-Skill aus Annotate installieren", "Instalar la habilidad de agente desde Annotate", "Встановлення навички агента з Annotate"]),
    ("Adds an Agents section to the Annotate pane that writes the hex-annotate skill into Claude Code, Codex, OpenCode, or Pi, and on macOS links hex onto PATH", ["Dodaje sekcję Agenci w panelu Annotate, która zapisuje umiejętność hex-annotate w Claude Code, Codex, OpenCode lub Pi, a w macOS dodaje hex do PATH", "在 Annotate 面板中加入“助手”部分，把 hex-annotate 技能写入 Claude Code、Codex、OpenCode 或 Pi，并在 macOS 上把 hex 加入 PATH", "Annotate パネルに「エージェント」セクションを追加し、hex-annotate スキルを Claude Code、Codex、OpenCode、Pi に書き込み、macOS では hex を PATH にリンクします", "Fügt dem Annotate-Bereich einen Abschnitt „Agenten“ hinzu, der den hex-annotate-Skill in Claude Code, Codex, OpenCode oder Pi schreibt und unter macOS hex in den PATH verlinkt", "Añade una sección Agentes al panel Annotate que escribe la habilidad hex-annotate en Claude Code, Codex, OpenCode o Pi, y en macOS enlaza hex en el PATH", "Додає розділ «Агенти» до панелі Annotate, який записує навичку hex-annotate у Claude Code, Codex, OpenCode або Pi, а на macOS додає hex до PATH"]),
];

/// Brazilian Portuguese, for the strings above.
#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Agents", "Agentes"),
    ("Let coding agents read your captures with the hex-annotate skill", "Deixe os agentes de código lerem suas capturas com a habilidade hex-annotate"),
    ("Not found", "Não encontrado"),
    ("Installed", "Instalado"),
    ("Update available", "Atualização disponível"),
    ("Managed elsewhere", "Gerenciado em outro lugar"),
    ("Install with the skills CLI", "Instalar com a CLI skills"),
    ("For skills you manage yourself", "Para habilidades que você mesmo gerencia"),
    ("Command-line tool", "Ferramenta de linha de comando"),
    ("Installed at {}", "Instalado em {}"),
    ("Owned by another program", "Pertence a outro programa"),
    ("{} isn't on your PATH", "{} não está no seu PATH"),
    ("Move Hex to Applications first", "Mova o Hex para Aplicativos primeiro"),
    ("The skill's zoom, inspect, and frames commands need Labs › Agent tools for captures on", "Os comandos zoom, inspect e frames da habilidade precisam do Labs › Ferramentas de agente para capturas"),
    ("Couldn't install the skill: {}", "Não foi possível instalar a habilidade: {}"),
    ("Couldn't link the command-line tool: {}", "Não foi possível vincular a ferramenta de linha de comando: {}"),
    ("Install agent skill from Annotate", "Instalar a habilidade de agente do Annotate"),
    ("Adds an Agents section to the Annotate pane that writes the hex-annotate skill into Claude Code, Codex, OpenCode, or Pi, and on macOS links hex onto PATH", "Adiciona uma seção Agentes ao painel Annotate que escreve a habilidade hex-annotate no Claude Code, Codex, OpenCode ou Pi e, no macOS, vincula o hex ao PATH"),
];

/// French, for the strings above.
#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Agents", "Agents"),
    ("Let coding agents read your captures with the hex-annotate skill", "Laissez les agents de code lire vos captures avec la compétence hex-annotate"),
    ("Not found", "Introuvable"),
    ("Installed", "Installé"),
    ("Update available", "Mise à jour disponible"),
    ("Managed elsewhere", "Géré ailleurs"),
    ("Install with the skills CLI", "Installer avec la CLI skills"),
    ("For skills you manage yourself", "Pour les compétences que vous gérez vous-même"),
    ("Command-line tool", "Outil en ligne de commande"),
    ("Installed at {}", "Installé dans {}"),
    ("Owned by another program", "Appartient à un autre programme"),
    ("{} isn't on your PATH", "{} n'est pas dans votre PATH"),
    ("Move Hex to Applications first", "Déplacez d'abord Hex dans Applications"),
    ("The skill's zoom, inspect, and frames commands need Labs › Agent tools for captures on", "Les commandes zoom, inspect et frames de cette compétence nécessitent Labs › Outils d'agent pour les captures"),
    ("Couldn't install the skill: {}", "Impossible d'installer la compétence : {}"),
    ("Couldn't link the command-line tool: {}", "Impossible de lier l'outil en ligne de commande : {}"),
    ("Install agent skill from Annotate", "Installer la compétence d'agent depuis Annotate"),
    ("Adds an Agents section to the Annotate pane that writes the hex-annotate skill into Claude Code, Codex, OpenCode, or Pi, and on macOS links hex onto PATH", "Ajoute une section Agents au panneau Annotate qui écrit la compétence hex-annotate dans Claude Code, Codex, OpenCode ou Pi, et sous macOS lie hex au PATH"),
];

/// Russian, for the strings above.
#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Agents", "Агенты"),
    ("Let coding agents read your captures with the hex-annotate skill", "Позвольте агентам для кода читать ваши снимки и записи с помощью навыка hex-annotate"),
    ("Not found", "Не найден"),
    ("Installed", "Установлено"),
    ("Update available", "Доступно обновление"),
    ("Managed elsewhere", "Управляется в другом месте"),
    ("Install with the skills CLI", "Установить через CLI skills"),
    ("For skills you manage yourself", "Для навыков, которыми управляете вы"),
    ("Command-line tool", "Инструмент командной строки"),
    ("Installed at {}", "Установлено в {}"),
    ("Owned by another program", "Принадлежит другой программе"),
    ("{} isn't on your PATH", "{} нет в вашем PATH"),
    ("Move Hex to Applications first", "Сначала переместите Hex в «Программы»"),
    ("The skill's zoom, inspect, and frames commands need Labs › Agent tools for captures on", "Команды zoom, inspect и frames этого навыка требуют включения Лаборатории › Инструменты агента для снимков"),
    ("Couldn't install the skill: {}", "Не удалось установить навык: {}"),
    ("Couldn't link the command-line tool: {}", "Не удалось связать инструмент командной строки: {}"),
    ("Install agent skill from Annotate", "Установка навыка агента из Annotate"),
    ("Adds an Agents section to the Annotate pane that writes the hex-annotate skill into Claude Code, Codex, OpenCode, or Pi, and on macOS links hex onto PATH", "Добавляет раздел «Агенты» на панель Annotate, который записывает навык hex-annotate в Claude Code, Codex, OpenCode или Pi, а на macOS добавляет hex в PATH"),
];
