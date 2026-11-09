<div align="center">

[English](README.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh.md) · **العربية** · [Русский](README.ru.md)

<a href="https://tahabenissa.github.io/winsploit/">
  <img src=".github/banner.svg" alt="Winsploit" width="720">
</a>

### صندوق أدوات الأمن الأصلي لويندوز، داخل طرفيتك.

**ابحث عن 45 أداة لاختبار الاختراق ومسابقات CTF وثبّتها وشغّلها من لوحة واحدة تُدار بالكامل عبر لوحة المفاتيح.**<br>
بلا WSL، بلا أجهزة افتراضية، وبلا تخمين لما يعمل فعلاً على ويندوز.

<br>

<a href="https://tahabenissa.github.io/winsploit/"><img src="https://img.shields.io/badge/Website-89c2d2?style=for-the-badge&logo=googlechrome&logoColor=0c191d" alt="Website" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/docs/"><img src="https://img.shields.io/badge/Docs-232831?style=for-the-badge&logo=readthedocs&logoColor=89c2d2" alt="Docs" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/demo/"><img src="https://img.shields.io/badge/Demo-232831?style=for-the-badge&logo=gnubash&logoColor=89c2d2" alt="Demo" height="36"></a>

<br>

[![CI](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml/badge.svg)](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/tahabenissa/winsploit?include_prereleases&sort=semver)](https://github.com/tahabenissa/winsploit/releases)
![platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6)
![tools](https://img.shields.io/badge/catalog-45%20tools-8A2BE2)
![license](https://img.shields.io/badge/license-MIT-green)

</div>

---

<div dir="rtl" align="right">

> [!WARNING]
> **للاستخدام المصرّح به فقط.** Winsploit مُخصّص لاختبار الاختراق المصرّح به، والبحث الأمني، ومسابقات
> CTF، والتعلّم. وجّهه فقط إلى أنظمة تملكها أو لديك **إذن كتابي صريح** باختبارها. المسح أو الهجوم غير
> المصرّح به غير قانوني في معظم الولايات القضائية، والبقاء ضمن قواعد الاشتباك الخاصة بك مسؤوليتك أنت.

## لماذا Winsploit؟

كالي هي موطن أدوات الأمن، لكن كثيرين يعملون على **ويندوز**، وكل الخيارات المتاحة تفرض عليك تنازلاً:

| الخيار | المميزات | ما ينقصه |
|---|---|---|
| **Commando VM / FLARE-VM** | ممتازة وشاملة | *أدوات تجهيز أجهزة افتراضية* لمرة واحدة، لا مُشغّل يومي |
| **PentestBox** | الفكرة الصحيحة: أصلي ومحمول | متوقّف منذ ~2016، ورخصته غير واضحة |
| **UniGetUI** | مدير حزم رسومي ممتاز | لا انتقاء أمني، ولا مسار «شغّل الأداة» |
| **Winsploit** | منتقى، أصلي، يثبّت **ويشغّل**، MIT، قيد تطوير نشط | إصدار ألفا |

لا يوجد مُشغّل أصلي لويندوز، مُصان وذو رخصة واضحة، يقوم في آنٍ واحد بـ*تثبيت* أدوات الأمن و*تشغيلها* من
واجهة واحدة. يسدّ Winsploit هذه الفجوة، وهو صريح بشأن ما يعمل فعلاً على ويندوز وما يحتاج إلى لينكس.

## البدء السريع

**الأسهل (بدون صلاحيات مسؤول):** نزّل `winsploit-<version>-setup.exe` من صفحة
[Releases](https://github.com/tahabenissa/winsploit/releases). المُثبِّت يضيفه إلى PATH، ويمكنه إعداد
Scoop وgit والـ buckets وpipx، ويتيح لك تحديد حزم أدوات. افتح طرفية **جديدة** واكتب `winsploit`.
الملف التنفيذي غير موقّع (سيعرض SmartScreen: «مزيد من المعلومات» ثم «تشغيل على أي حال»)؛ تحقّق منه
عبر `SHA256SUMS`.

**أو البناء من المصدر:**

**تحتاج إلى:** Windows 10/11، و[Scoop](https://scoop.sh)، و[Rust 1.87+](https://rustup.rs).

</div>

```powershell
# 1. تثبيت Scoop (تخطَّ إن كان موجوداً)
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

# 2. بناء winsploit
git clone https://github.com/tahabenissa/winsploit
cd winsploit
cargo build --release

# 3. جعل `winsploit` أمراً (يضيف target\release إلى PATH بدون صلاحيات مسؤول)
.\target\release\winsploit setup --path --shortcut

# 4. افتح طرفية جديدة، انتقل إلى مجلد عملك، وشغّله
cd C:\ctf\some-box
winsploit
```

<div dir="rtl" align="right">

اختر أداة بـ `↑/↓` (أو `/` للتصفية)، اضغط `i` لتثبيتها و`Enter` لتشغيلها. اضغط `?` في أي وقت لعرض كل المفاتيح.

## المزايا

### لوحة مصمّمة للوحة المفاتيح
- **تصفّح حسب الفئة** مع حالة التثبيت مباشرة: `●` مُثبَّت، `○` مفقود، `◐` تعارض.
- **تصفية فورية (`/`)** بالاسم أو الفئة أو الوصف.
- **محرّر أسطر حقيقي** للوسائط، مع **سجلّ** لكل أداة (`↑/↓`).
- **مساعدة المفاتيح (`?`)**.

### مخرجات حيّة وتفاعلية
- يُبثّ stdout/stderr **إلى لوحة الإخراج** فوراً، مع ألوان.
- **حاوِر الأدوات أثناء تشغيلها.** لكل عملية أنبوب إدخال خاص (`Ctrl+D` = نهاية الإدخال).
- **تمرير** (`PgUp/PgDn/Home/End`) و**بحث في الإخراج** (`f`، `n`، `N`).
- **`c` يلغي** شجرة العمليات كاملة، لا الأب فقط.

### تثبيت بضغطة واحدة، بإتقان
- **Scoop** للأدوات الأصلية، مع **إضافة الـ buckets الناقصة تلقائياً** عند الحاجة.
- **pipx** لأدوات بايثون (لكل واحدة بيئتها؛ ويُثبَّت بايثون وpipx عند غيابهما).
- **bucket خاص بـ Winsploit** للأدوات التي لا يحزمها أحد، مع **تحقّق SHA-256**.
- يجري الكشف **خارج خيط الواجهة**، فلا تتجمّد اللوحة أبداً.

### مصمّم لمسابقات CTF
- **مساحات التحدّي (`C`)**: مجلد لكل تحدٍّ يضم الهدف وصيغة الـ flag والملفات والملاحظات والسجلات والأعلام.
- **الوصفات**: `Enter` على nmap وffuf وjohn… يعرض أوامر جاهزة مع `{target}` و`{file}` و`{out}` و`{wordlist}` مملوءة.
- **ملتقِط الأعلام**: تُبرز `picoCTF{…}` و`HTB{…}` و`flag{…}` ويُعلَن عنها وتُنسخ وتُحفظ.

### وكيل ذكاء اصطناعي ضمن الحلقة
- اضغط **`a`**: يقرأ الذكاء الاصطناعي مخرجات الطرفية ويقترح الخطوات التالية على شكل **أوامر لأدوات من
  الكتالوج تؤكّدها قبل تشغيلها**. الوكيل نفسه لا يشغّل أي صدفة (shell).
- **`A`** يختار المزوّد: **Claude CLI** (يعيد استخدام جلستك، بلا مفتاح)، أو **Gemini / DeepSeek / OpenAI**،
  أو نموذج **Ollama** محلي — مع إمكان تخصيص النموذج لكل مزوّد. تُقرأ المفاتيح من البيئة.

### الجلسة نفسها في متصفّحك
تبقى الطرفية التطبيق الرئيسي، والمتصفّح نافذة ثانية على **الجلسة الحيّة نفسها**. `w` يبدأ/يوقف عرض الويب،
و`o` يفتحه. **`--lan`** (أو `W`) يقدّمه لشبكتك المحلية **عبر HTTPS**، بمفتاح دخول لمرة واحدة وبموافقتك لكل ضيف.

### مدفوعة بالإعداد، صريحة بالتصميم
- كل أداة هي **مُدخَل YAML**، فتضيف واحدة دون لمس الشيفرة.
- **كشف المنتحِلين**: يلتقط `verify:` البرامج ذات الاسم نفسه ويرفض تشغيل الخاطئ.

## الكتالوج

**45 أداة عبر 11 فئة**، اختيرت جميعها لأنها تعمل جيداً **بشكل أصلي** على ويندوز.

| الفئة | الأدوات |
|---|---|
| **الاستطلاع** | nmap · rustscan · gobuster · httpx · subfinder · amass · gau |
| **الويب** | sqlmap · nuclei · ffuf · feroxbuster · katana · arjun · dirsearch · wfuzz · commix · nikto · ZAP (GUI) |
| **كلمات المرور** | hashcat · John the Ripper (jumbo) · hashid · NetExec |
| **التشفير** | openssl · RsaCtfTool · CyberChef (المتصفّح) |
| **التحليل الجنائي** | exiftool · strings (Sysinternals) · Sleuth Kit · Volatility 3 |
| **الهندسة العكسية** | radare2 · upx · jadx · apktool · Detect It Easy · Cutter (GUI) · x64dbg (GUI) · dnSpy (GUI) · Ghidra (GUI) |
| **Pwn** | pwntools · ropper |
| **OSINT** | theHarvester · Sherlock |
| **إخفاء المعلومات** | steghide |
| **التقاط الحزم** | tshark |
| **قوائم الكلمات** | SecLists-lite (بيانات) |

> [!IMPORTANT]
> **عن اللاسلكي بصراحة:** التقاط Wi-Fi بوضع المراقبة وحقن الحزم **لا يعملان** على ويندوز بالمحوّلات
> العادية. المفيد هنا هو فقط الكسر *دون اتصال* لملفات التقطتها مسبقاً. للعمل اللاسلكي الحقيقي استخدم
> لينكس مع محوّل USB مدعوم.

## الاستخدام

</div>

```text
winsploit                 تشغيل اللوحة التفاعلية
winsploit list            سرد الكتالوج مع حالة التثبيت
winsploit hashid <h>…     التعرّف على هاش أو أكثر (تلميحات hashcat -m / john --format)
winsploit --help, -h      عرض المساعدة
winsploit --version, -V   عرض الإصدار
```

<div dir="rtl" align="right">

### مفاتيح اللوحة

| المفتاح | الإجراء |
|---|---|
| `↑` `↓` | تحريك التحديد |
| `/` | التصفية بالاسم أو الفئة أو الوصف |
| `Enter` | تشغيل الأداة المحدّدة (حرّر الوسائط ثم `Enter`) |
| `i` | تثبيت الأداة (Scoop أو pipx) |
| `r` | إعادة كشف المثبّت |
| `f` · `n` · `N` | بحث في الإخراج · السابق · التالي |
| `c` | إلغاء العملية الجارية (الشجرة كاملة) |
| `h` | التعرّف على هاش (مدمج، دون اتصال) |
| `a` · `A` | وكيل الذكاء الاصطناعي: قراءة الإخراج واقتراح الخطوات · اختيار المزوّد / النموذج |
| `p` | المشاريع: إنشاء / فتح مجلد عمل، مع عناصر حديثة |
| `d` | مجلد العمل: حيث تعمل الأدوات، مثل `cd` |
| `C` | التحدّيات: إنشاء، تبديل، ضبط الهدف / صيغة الـ flag |
| `F` | الأعلام الملتقَطة من الإخراج |
| `S` | تشغيل الأداة داخل Windows Sandbox |
| `t` | المظهر: السمة، الحدود، الرموز (معاينة حيّة) |
| `w` · `W` · `o` | عرض الويب: بدء / تقديم للشبكة المحلية / فتح |
| `?` | عرض كل المفاتيح |
| `q` | خروج |

---

📖 **المرجع الكامل** (نموذج الأمن، البنية، إضافة أداة، الوصفات، الصندوق الرملي…): راجع
**[README بالإنجليزية](README.md)** فهو النسخة المرجعية.

## المساهمة

إضافة الأدوات هي أسهل بداية: أضف مُدخَلاً في [`tools.yaml`](tools.yaml) (ومانيفست في `bucket/` إن لم
يحزمها أحد)، شغّل `cargo test`، وافتح PR. أضف فقط أدوات تعمل فعلاً بشكل أصلي على ويندوز، ووثّق أي
تحفّظات عبر `note:`.

## الرخصة

[MIT](LICENSE) © مساهمو Winsploit. كل أداة مُرفقة أو مُثبّتة تحتفظ برخصتها الخاصة.

</div>
