//! Traditional Chinese (zh-Hant) interface strings, keyed by the English
//! source string like [`super::TRANSLATIONS`]. Each string is the
//! Simplified Chinese one converted with the checked-in tables below:
//! Taiwan wording first ([`PHRASES`]: 設定, 預設, 檔案, 快速鍵, 貼上),
//! then character by character ([`CHARACTERS`]), so a new Simplified string
//! is never left untranslated here. Characters the table doesn't know pass
//! through unchanged. Not yet reviewed by a native speaker.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The Traditional Chinese strings by their English source.
pub(super) fn table() -> &'static HashMap<&'static str, String> {
    static TABLE: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let simplified = super::column_for_code("zh") as usize;
        super::TRANSLATIONS
            .iter()
            .map(|(english, translated)| (*english, convert(translated[simplified])))
            .collect()
    })
}

/// Simplified Chinese text in Traditional characters and Taiwan wording.
fn convert(simplified: &str) -> String {
    let phrases = phrases();
    let characters = characters();
    let input: Vec<char> = simplified.chars().collect();
    let mut output = String::with_capacity(simplified.len());
    let mut index = 0;
    'next: while index < input.len() {
        for (from, to) in phrases {
            if input[index..].starts_with(from) {
                output.push_str(to);
                index += from.len();
                continue 'next;
            }
        }
        let character = input[index];
        output.push(characters.get(&character).copied().unwrap_or(character));
        index += 1;
    }
    output
}

/// [`PHRASES`], longest first, so `文件夹` wins over `文件`.
fn phrases() -> &'static [(Vec<char>, &'static str)] {
    static PHRASE_LIST: OnceLock<Vec<(Vec<char>, &'static str)>> = OnceLock::new();
    PHRASE_LIST.get_or_init(|| {
        let mut list: Vec<_> = PHRASES
            .iter()
            .map(|(from, to)| (from.chars().collect::<Vec<_>>(), *to))
            .collect();
        list.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        list
    })
}

fn characters() -> &'static HashMap<char, char> {
    static MAP: OnceLock<HashMap<char, char>> = OnceLock::new();
    MAP.get_or_init(|| character_pairs().collect())
}

/// Each `简繁` token of [`CHARACTERS`] as a pair; malformed tokens are
/// skipped (and fail `character_tokens_are_well_formed`).
fn character_pairs() -> impl Iterator<Item = (char, char)> {
    CHARACTERS
        .iter()
        .flat_map(|line| line.split_whitespace())
        .filter_map(|token| {
            let mut characters = token.chars();
            match (characters.next(), characters.next(), characters.next()) {
                (Some(simplified), Some(traditional), None) => Some((simplified, traditional)),
                _ => None,
            }
        })
}

/// Simplified words with a different Traditional (Taiwan) wording or a
/// character whose conversion depends on the word.
#[rustfmt::skip]
const PHRASES: &[(&str, &str)] = &[
    ("文件夹", "資料夾"), ("文件", "檔案"), ("文档", "文件"),
    ("应用程序", "應用程式"), ("驱动程序", "驅動程式"), ("驱动", "驅動程式"),
    ("程序坞", "Dock"), ("程序", "程式"), ("进程", "處理程序"),
    ("信息", "資訊"), ("消息", "訊息"), ("支持", "支援"), ("运行", "執行"),
    ("质量", "品質"), ("视频", "影片"), ("音频", "音訊"), ("屏幕", "螢幕"),
    ("鼠标", "滑鼠"), ("光标", "游標"), ("显存", "顯示記憶體"), ("内存", "記憶體"),
    ("硬件", "硬體"), ("软件", "軟體"), ("默认", "預設"), ("设置", "設定"),
    ("粘贴", "貼上"), ("剪贴板", "剪貼簿"), ("快捷键", "快速鍵"), ("快捷方式", "捷徑"),
    ("菜单栏", "選單列"), ("菜单", "選單"), ("窗口", "視窗"), ("导入", "匯入"),
    ("导出", "匯出"), ("加载", "載入"), ("实时", "即時"), ("网络", "網路"),
    ("搜索", "搜尋"), ("识别", "辨識"), ("检测", "偵測"), ("设备", "裝置"),
    ("数据", "資料"), ("字符串", "字串"), ("字符", "字元"), ("文本", "文字"),
    ("用户", "使用者"), ("服务器", "伺服器"), ("代码", "程式碼"), ("命令", "指令"),
    ("终端", "終端機"), ("缓存", "快取"), ("磁盘", "磁碟"), ("摄像头", "攝影機"),
    ("分辨率", "解析度"), ("系统托盘", "系統匣"), ("托盘", "系統匣"), ("图标", "圖示"),
    ("创建", "建立"), ("保存", "儲存"), ("链接", "連結"), ("界面", "介面"),
    ("通过", "透過"), ("兼容", "相容"), ("激活", "啟用"), ("登录项", "登入項目"),
    ("登录", "登入"), ("账户", "帳戶"), ("帐户", "帳戶"), ("标签页", "分頁"),
    ("打印", "列印"), ("视图", "檢視"), ("优化", "最佳化"), ("互联网", "網際網路"),
    ("在线", "線上"), ("信号", "訊號"), ("首选项", "偏好設定"), ("字体", "字型"),
    ("线程", "執行緒"), ("高级", "進階"), ("自定义", "自訂"), ("重置", "重設"),
    ("刷新", "重新整理"), ("撤销", "復原"), ("复制", "複製"), ("重复", "重複"),
    ("复杂", "複雜"), ("回复", "回覆"), ("复选", "複選"), ("后台", "背景"),
    ("干净", "乾淨"), ("关系", "關係"), ("联系", "聯絡"), ("录制", "錄製"),
    ("制作", "製作"), ("绘制", "繪製"), ("二进制", "二進位"), ("日历", "行事曆"),
    ("游戏", "遊戲"), ("占用", "佔用"), ("注册", "註冊"), ("注释", "註解"),
    ("辅助功能", "輔助使用"), ("显卡", "顯示卡"), ("词汇", "詞彙"), ("添加", "新增"),
    ("智能", "智慧"), ("退出", "結束"), ("禁用", "停用"), ("反馈", "回饋"),
    ("发送", "傳送"), ("打开", "開啟"), ("重命名", "重新命名"), ("废纸篓", "垃圾桶"),
    ("内置", "內建"), ("合并", "合併"), ("汇总", "彙總"),
];

/// Simplified-to-Traditional characters, one `简繁` pair per token, for the
/// characters whose Traditional form differs. Taiwan quotation marks too.
#[rustfmt::skip]
const CHARACTERS: &[&str] = &[
    "这這 们們 个個 来來 时時 对對 发發 会會 后後 过過 进進 还還 样樣 动動 种種 实實 学學 么麼 两兩 应應",
    "长長 开開 关關 东東 头頭 机機 电電 万萬 从從 无無 与與 为為 产產 业業 书書 国國 众眾 体體 当當 于於",
    "着著 里裡 将將 并並 几幾 处處 义義 尔爾 乐樂 习習 买買 乱亂 争爭 亚亞 亲親 仅僅 仓倉 优優 价價 传傳",
    "伤傷 伦倫 声聲 备備 复復 够夠 夺奪 奋奮 妇婦 娱娛 孙孫 宁寧 宝寶 审審 宪憲 寻尋 导導 层層 届屆 属屬",
    "岁歲 岛島 峡峽 币幣 师師 帐帳 带帶 帮幫 广廣 庄莊 庆慶 废廢 异異 弃棄 张張 弹彈 归歸 录錄 彻徹 征徵",
    "忆憶 忧憂 态態 怀懷 总總 恋戀 恶惡 悬懸 惊驚 惯慣 愿願 戏戲 战戰 户戶",
    "扑撲 执執 扩擴 扫掃 扬揚 扰擾 抚撫 抛拋 抢搶 护護 报報 担擔 拟擬 拥擁 拦攔 择擇 挂掛 挡擋 挤擠 挥揮",
    "损損 捡撿 换換 据據 掷擲 揽攬 搅攪 携攜 摄攝 摆擺 摇搖 撑撐 敌敵 数數 斋齋 断斷 旧舊 显顯 晓曉 晒曬",
    "晕暈 暂暫 术術 杀殺 杂雜 权權 条條 杨楊 极極 构構 枪槍 柜櫃 标標 栈棧 栋棟 栏欄 树樹 档檔 桥橋 检檢",
    "楼樓 横橫 欢歡 欧歐 残殘 毕畢 气氣 汉漢 汤湯 沟溝 没沒 泪淚 泽澤 洁潔 洒灑 浅淺 测測 济濟 浏瀏 浓濃",
    "涂塗 润潤 涨漲 渊淵 渔漁 温溫 湾灣 湿濕 满滿 滚滾 滤濾 滥濫 灭滅 灯燈 灵靈 灾災 炉爐 点點 炼煉 烂爛",
    "烟煙 烦煩 烧燒 热熱 焕煥 爱愛 爷爺 牵牽 犹猶 状狀 独獨 狭狹 狮獅 猎獵 猪豬 献獻 环環 现現 琐瑣 画畫",
    "畅暢 疗療 疯瘋 痒癢 盖蓋 盘盤 矿礦 础礎 硕碩 确確 碍礙 礼禮 祸禍 离離 积積 称稱 稳穩 穷窮 窃竊 窍竅",
    "竖豎 竞競 笔筆 笼籠 筑築 签簽 简簡 类類 粮糧 紧緊 网網 罗羅 罚罰 罢罷 职職 联聯 聪聰 肃肅 肠腸 肤膚",
    "肿腫 胀脹 胜勝 胆膽 脉脈 脑腦 脏髒 脚腳 脱脫 腊臘 舍捨 舱艙 艺藝 节節 芜蕪 苍蒼 苏蘇 范範 荐薦 荣榮",
    "药藥 莱萊 获獲 虑慮 虚虛 虫蟲 补補 衬襯 袭襲 装裝 触觸 誉譽 赶趕 趋趨 跃躍 践踐 踪蹤 边邊 辽遼 达達",
    "迁遷 迈邁 运運 远遠 迟遲 适適 选選 递遞 逻邏 遗遺 邓鄧 邮郵 邻鄰 郑鄭 酱醬 酿釀 释釋 鉴鑒 阳陽 阴陰",
    "阶階 际際 陆陸 陈陳 险險 随隨 隐隱 隶隸 难難 雾霧 韩韓 飞飛 齐齊 齿齒 龙龍 龟龜",
    "办辦 务務 劝勸 劳勞 势勢 勋勛 区區 医醫 华華 协協 单單 卖賣 卢盧 卫衛 却卻 厂廠 厅廳 历歷 压壓 厌厭",
    "厕廁 县縣 参參 双雙 变變 叙敘 叠疊 号號 叹嘆 吓嚇 启啟 员員 呜嗚 响響 哑啞 哗嘩 唤喚 啰囉 喷噴 嘱囑",
    "团團 园園 图圖 圆圓 圣聖 场場 坏壞 块塊 坚堅 坛壇 坝壩 坟墳 坠墜 垄壟 垒壘 墙牆 壮壯 壳殼 奖獎 妆妝",
    "婴嬰 宠寵 宾賓 寝寢 寿壽 尘塵 尝嘗 尧堯 尸屍 尽盡 屉屜 屡屢 岂豈 岗崗 岭嶺 峦巒 巩鞏 帅帥 帘簾 帜幟",
    "帧幀 幂冪 庐廬 庙廟 庞龐 弯彎 弥彌 彦彥 径徑 忏懺 怜憐 恳懇 恼惱 悦悅 惧懼 惨慘 惩懲 愤憤 慑懾 懒懶",
    "抠摳 抡掄 拣揀 拢攏 挚摯 挛攣 挠撓 挣掙 捞撈 捣搗 掳擄 掴摑 掸撣 搀攙 搁擱 搂摟 摊攤 撵攆",
    "亿億 仪儀 伞傘 伪偽 佣傭 侠俠 侣侶 侥僥 侦偵 侧側 侨僑 俭儉 债債 倾傾 偿償 储儲 儿兒 兑兌 党黨 兰蘭",
    "兴興 养養 兽獸 冈岡 册冊 写寫 农農 冯馮 冲衝 决決 况況 冻凍 净淨 凉涼 减減 凑湊 凤鳳 凭憑 凯凱 击擊",
    "凿鑿 刘劉 则則 刚剛 创創 删刪 别別 刹剎 剂劑 剑劍 剧劇 剥剝 劲勁 乌烏 乔喬 亏虧 云雲 亩畝 仆僕 丛叢",
    "丝絲 丢丟 严嚴 丧喪 临臨 丽麗 举舉 乡鄉 荡蕩 莲蓮 萝蘿 营營 萧蕭 萨薩 蓝藍 蔼藹 蕴蘊 灿燦 炖燉 烛燭",
    "烫燙 烬燼 旷曠 昼晝 晋晉 晖暉 暧曖 杰傑 枣棗 枢樞 枫楓 柠檸 栅柵 桨槳 桩樁 梦夢 椭橢 榄欖 樱櫻 歼殲",
    "殴毆 毁毀 毙斃 毡氈 氢氫 汇匯 沥瀝 沦淪 沧滄 泞濘 泻瀉 泼潑 洼窪 浆漿 浇澆 浊濁 浑渾 涌湧 涛濤 涡渦",
    "涤滌 涩澀 渍漬 渐漸 渗滲 溃潰 溅濺 滞滯 滨濱 滩灘 潇瀟 潜潛 澜瀾 濒瀕 狈狽 狞獰 狱獄 猫貓 玺璽 琼瓊",
    "疟瘧 疮瘡 瘫癱 瘾癮 癣癬 皑皚 皱皺 盏盞 盐鹽 监監 盗盜 睁睜 瞒瞞 瞩矚 矫矯 矶磯 砖磚 砚硯 碱鹼 祯禎",
    "禀稟 秃禿 秽穢 稣穌 窑窯 窜竄 窝窩 窥窺 笃篤 笋筍 笺箋 筛篩 筹籌 箩籮 篮籃 篱籬 粪糞 羡羨 翘翹 耸聳",
    "耻恥 聂聶 聋聾 肮骯 肾腎 胁脅 胶膠 脐臍 脓膿 脸臉 腻膩 腾騰 舆輿 艰艱 艳艷 芦蘆 苹蘋 茎莖 荚莢 莹瑩",
    "萤螢 虏虜 虽雖 蚀蝕 蚁蟻 蛮蠻 蜗蝸 蝇蠅 蝉蟬 衔銜 袄襖 袜襪 踊踴 蹑躡 蹿躥 躯軀 迹跡 逊遜 遥遙 酝醞",
    "队隊 陇隴 陕陝 陨隕 雏雛 雳靂 麦麥 龄齡",
    "计計 订訂 认認 讨討 让讓 训訓 议議 讯訊 记記 讲講 讳諱 讶訝 许許 论論 讼訟 设設 访訪 证證 评評 识識",
    "诈詐 诉訴 词詞 译譯 试試 诗詩 诚誠 话話 诞誕 询詢 该該 详詳 语語 误誤 说說 请請 诸諸 诺諾 读讀 课課",
    "谁誰 调調 谈談 谊誼 谋謀 谎謊 谐諧 谓謂 谜謎 谢謝 谣謠 谦謙 谨謹 谱譜 谬謬 讽諷 诀訣 诊診 诵誦 诱誘",
    "诲誨 谅諒 谴譴 谭譚 讹訛 讥譏 诡詭 诠詮 诅詛 诫誡 诬誣 谍諜 谏諫 谘諮 谙諳 谚諺 谤謗 谧謐 诧詫 诣詣",
    "诘詰 诙詼 谕諭 谒謁 谗讒 谛諦",
    "针針 钉釘 钓釣 钙鈣 钝鈍 钞鈔 钟鐘 钢鋼 钥鑰 钦欽 钧鈞 钩鉤 钮鈕 钱錢 钳鉗 钻鑽 铁鐵 铃鈴 铅鉛 铜銅",
    "铝鋁 铭銘 银銀 铺鋪 链鏈 销銷 锁鎖 锄鋤 锅鍋 锈鏽 锋鋒 锌鋅 锐銳 错錯 锚錨 锡錫 锣鑼 锤錘 锦錦 键鍵",
    "锯鋸 镇鎮 镜鏡 镑鎊 镶鑲 铸鑄 铲鏟 钾鉀 钠鈉 铬鉻 镁鎂 铀鈾 锂鋰 钛鈦 钨鎢 镍鎳 铂鉑 锻鍛 镀鍍 镰鐮",
    "钵缽 锭錠",
    "饥飢 饭飯 饮飲 饰飾 饱飽 饲飼 饺餃 饼餅 饿餓 馆館 馈饋 馒饅 饶饒 馅餡 馋饞",
    "纠糾 红紅 纤纖 约約 级級 纪紀 纬緯 纯純 纱紗 纲綱 纳納 纵縱 纷紛 纸紙 纹紋 纺紡 纽紐 线線 练練 组組",
    "细細 织織 终終 绍紹 经經 绑綁 绒絨 结結 绕繞 绘繪 给給 络絡 绝絕 统統 绢絹 绣繡 继繼 绩績 绪緒 续續",
    "绳繩 维維 绵綿 绸綢 综綜 绿綠 缀綴 缓緩 编編 缘緣 缚縛 缝縫 缠纏 缩縮 缴繳 绅紳 绞絞 绰綽 绷繃 缅緬",
    "缆纜 缎緞 缔締 缕縷 缤繽 缭繚 纫紉 纶綸 绎繹 绊絆 绽綻 缨纓 缮繕",
    "门門 闪閃 闭閉 问問 闯闖 闲閒 间間 闷悶 闸閘 闹鬧 闻聞 阀閥 阁閣 阅閱 阐闡 阔闊 阙闕 阎閻 闰閏 阑闌",
    "阂閡 闺閨 阈閾",
    "贝貝 负負 贡貢 财財 责責 贤賢 败敗 账帳 货貨 质質 贩販 贪貪 贫貧 购購 贯貫 贱賤 贴貼 贵貴 贷貸 贸貿",
    "费費 贺賀 资資 赋賦 赌賭 赏賞 赐賜 赔賠 赖賴 赚賺 赛賽 赞贊 赠贈 赢贏 赵趙 贾賈 赃贓 贿賄 赂賂 赁賃",
    "赎贖 赘贅",
    "见見 观觀 规規 视視 览覽 觉覺 舰艦 宽寬 觅覓",
    "车車 轧軋 轨軌 军軍 轩軒 转轉 轮輪 软軟 轰轟 轴軸 轻輕 载載 较較 辅輔 辆輛 辈輩 输輸 辐輻 辑輯 辖轄",
    "阵陣 连連 库庫 裤褲 斩斬 惭慚 堑塹",
    "马馬 驮馱 驯馴 驰馳 驱驅 驳駁 驴驢 驶駛 驻駐 驼駝 驾駕 驿驛 骂罵 骄驕 验驗 骑騎 骗騙 骚騷 骤驟 吗嗎",
    "妈媽 码碼 玛瑪 蚂螞 骆駱 骏駿 骇駭",
    "鸟鳥 鸡雞 鸣鳴 鸭鴨 鸽鴿 鹅鵝 鹰鷹 鹤鶴 鸦鴉 鱼魚 鲁魯 鲜鮮 鲸鯨 鳞鱗",
    "页頁 顶頂 项項 顺順 须須 顽頑 顾顧 顿頓 颁頒 预預 领領 颇頗 频頻 颗顆 题題 额額 颜顏 颠顛 颤顫 颈頸",
    "颊頰 颂頌 颖穎 颓頹 颅顱 风風 飘飄 韦韋 违違 围圍 伟偉 苇葦 韧韌",
    "听聽 专專 准準 余餘 内內 静靜 夹夾 烁爍 俩倆 拨撥 卧臥 夸誇 尴尷 松鬆 采採 划劃 牺犧",
    "“「 ”」",
];

#[cfg(test)]
mod tests {
    use super::{CHARACTERS, PHRASES, character_pairs, convert, table};

    #[test]
    fn character_tokens_are_well_formed() {
        let mut seen = std::collections::HashSet::new();
        let mut tokens = 0;
        for token in CHARACTERS.iter().flat_map(|line| line.split_whitespace()) {
            let characters: Vec<char> = token.chars().collect();
            assert_eq!(characters.len(), 2, "malformed token {token:?}");
            assert_ne!(characters[0], characters[1], "{token:?} maps to itself");
            assert!(seen.insert(characters[0]), "{token:?} is listed twice");
            tokens += 1;
        }
        assert_eq!(character_pairs().count(), tokens);
    }

    #[test]
    fn phrases_are_unique_simplified_words() {
        let mut seen = std::collections::HashSet::new();
        for (from, to) in PHRASES {
            assert!(seen.insert(from), "duplicate phrase {from:?}");
            assert!(!to.is_empty());
        }
    }

    #[test]
    fn conversion_prefers_taiwan_wording_then_characters() {
        assert_eq!(convert("设置默认快捷键"), "設定預設快速鍵");
        assert_eq!(convert("打开文件夹中的文件"), "開啟資料夾中的檔案");
        assert_eq!(convert("粘贴上次听写"), "貼上上次聽寫");
        assert_eq!(convert("从菜单栏“粘贴”"), "從選單列「貼上」");
        assert_eq!(convert("移除 {}？"), "移除 {}？");
        assert_eq!(
            convert("{model} 已被语言筛选隐藏"),
            "{model} 已被語言篩選隱藏"
        );
    }

    #[test]
    fn every_string_has_a_traditional_translation() {
        let simplified = super::super::column_for_code("zh") as usize;
        for (english, translated) in super::super::TRANSLATIONS {
            let traditional = table().get(english).expect("every string converts");
            assert!(!traditional.is_empty(), "{english:?} is empty");
            assert_eq!(
                traditional.matches('{').count(),
                translated[simplified].matches('{').count(),
                "{english:?} lost a placeholder"
            );
        }
    }
}
