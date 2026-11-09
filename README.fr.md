<div align="center">

[English](README.md) · **Français** · [Español](README.es.md) · [中文](README.zh.md) · [العربية](README.ar.md) · [Русский](README.ru.md)

<a href="https://tahabenissa.github.io/winsploit/">
  <img src=".github/banner.svg" alt="Winsploit" width="720">
</a>

### La boîte à outils de sécurité native Windows, dans votre terminal.

**Trouvez, installez et lancez 45 outils de pentest & CTF depuis un tableau de bord piloté au clavier.**<br>
Pas de WSL. Pas de VM. Aucune incertitude sur ce qui marche vraiment sous Windows.

<br>

<a href="https://tahabenissa.github.io/winsploit/"><img src="https://img.shields.io/badge/Site%20web-89c2d2?style=for-the-badge&logo=googlechrome&logoColor=0c191d" alt="Site web" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/docs/"><img src="https://img.shields.io/badge/Docs-232831?style=for-the-badge&logo=readthedocs&logoColor=89c2d2" alt="Docs" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/demo/"><img src="https://img.shields.io/badge/D%C3%A9mo-232831?style=for-the-badge&logo=gnubash&logoColor=89c2d2" alt="Démo" height="36"></a>

<br>

[![CI](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml/badge.svg)](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/tahabenissa/winsploit?include_prereleases&sort=semver)](https://github.com/tahabenissa/winsploit/releases)
![platform](https://img.shields.io/badge/plateforme-Windows%2010%20%7C%2011-0078D6)
![tools](https://img.shields.io/badge/catalogue-45%20outils-8A2BE2)
![license](https://img.shields.io/badge/licence-MIT-green)

</div>

---

> [!WARNING]
> **Usage autorisé uniquement.** Winsploit est destiné aux tests d'intrusion autorisés, à la
> recherche en sécurité, aux CTF et à l'apprentissage. Ne le pointez que vers des systèmes qui vous
> appartiennent ou pour lesquels vous avez une **autorisation écrite explicite**. Le scan ou
> l'attaque non autorisés sont illégaux dans la plupart des juridictions, et il est de votre
> responsabilité de rester dans le cadre de vos règles d'engagement.

---

## Pourquoi Winsploit ?

Kali est la référence des outils de sécurité, mais beaucoup de gens travaillent sous **Windows**, et
les options existantes imposent toutes un compromis :

| Option | Les plus | Les manques |
|---|---|---|
| **Commando VM / FLARE-VM** | Excellents, complets | *Provisionneurs de VM* à usage unique, pas un outil du quotidien |
| **PentestBox** | La bonne idée : natif et portable | Abandonné depuis ~2016, licence floue |
| **UniGetUI** | Très bon gestionnaire de paquets | Aucune curation sécurité, pas de workflow « lancer l'outil » |
| **Winsploit** | Curé, natif, installe **et** lance, MIT, activement développé | Alpha |

Winsploit comble ce manque : un lanceur natif Windows, maintenu et sous licence claire, qui à la
fois *installe* et *lance* les outils de sécurité depuis une seule interface — et qui est honnête
sur ce qui marche vraiment sous Windows et ce qui nécessite Linux.

## Démarrage rapide

**Le plus simple (sans admin) :** téléchargez `winsploit-<version>-setup.exe` depuis la page
[Releases](https://github.com/tahabenissa/winsploit/releases). L'installeur l'ajoute à votre PATH,
peut configurer Scoop, git, les buckets et pipx, et propose des packs d'outils à cocher. Ouvrez un
**nouveau** terminal et tapez `winsploit`. Le binaire n'est pas signé (SmartScreen : « Informations
complémentaires » puis « Exécuter quand même ») ; vérifiez-le avec `SHA256SUMS`.

**Ou compiler depuis les sources :**

**Il vous faut :** Windows 10/11, [Scoop](https://scoop.sh) et [Rust 1.87+](https://rustup.rs).

```powershell
# 1. Installer Scoop (sauter si déjà fait)
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

# 2. Compiler winsploit
git clone https://github.com/tahabenissa/winsploit
cd winsploit
cargo build --release

# 3. Faire de `winsploit` une commande (ajoute target\release à votre PATH, sans admin)
.\target\release\winsploit setup --path --shortcut

# 4. Ouvrir un nouveau terminal, aller où vous travaillez, et lancer
cd C:\ctf\une-box
winsploit
```

Choisissez un outil avec `↑/↓` (ou `/` pour filtrer), pressez `i` pour l'installer et `Entrée` pour
le lancer. Pressez `?` à tout moment pour voir toutes les touches.

## Fonctionnalités

### Un tableau de bord pensé pour le clavier
- **Parcours par catégorie** avec statut d'installation en direct : `●` installé, `○` manquant, `◐` conflit.
- **Filtre instantané (`/`)** par nom, catégorie ou description.
- **Un vrai éditeur de ligne** pour les arguments, avec **historique** par outil (`↑/↓`).
- **Aide des touches (`?`)**.

### Sortie en direct et interactive
- stdout/stderr **diffusés dans le volet** en temps réel, colorés.
- **Dialoguez avec les outils en cours.** Chaque processus a son propre canal d'entrée (`Ctrl+D` = fin d'entrée).
- **Défilement** (`PgUp/PgDn/Home/End`) et **recherche dans la sortie** (`f`, `n`, `N`).
- **`c` annule** tout l'arbre de processus, pas seulement le parent.

### Installations en une touche, bien faites
- **Scoop** pour les outils natifs, avec **ajout automatique des buckets** manquants.
- **pipx** pour les outils Python (chacun dans son venv ; Python et pipx installés au besoin).
- **Le bucket Scoop de Winsploit** pour les outils que personne n'empaquette, avec **hash SHA-256**.
- Détection **hors du thread UI** : le tableau de bord ne gèle jamais.

### Pensé pour les CTF
- **Espaces de challenge (`C`)** : un dossier par challenge avec sa cible, le format de flag, les fichiers, les notes, les logs et les flags.
- **Recettes** : `Entrée` sur nmap, ffuf, john… propose des commandes prêtes avec `{target}`, `{file}`, `{out}`, `{wordlist}` remplis.
- **Capteur de flags** : `picoCTF{…}`, `HTB{…}`, `flag{…}` surlignés, annoncés, copiés et enregistrés.

### Un agent IA dans la boucle
- Pressez **`a`** : une IA lit la sortie du terminal et propose les étapes suivantes sous forme de
  **commandes d'outils du catalogue que vous confirmez avant exécution**. L'agent ne lance jamais de
  shell lui-même.
- **`A`** choisit le fournisseur : **CLI Claude** (réutilise votre session, sans clé), ou
  **Gemini / DeepSeek / OpenAI**, ou un modèle **Ollama** local — avec un modèle personnalisable par
  fournisseur. Les clés sont lues depuis l'environnement.

### La même session dans votre navigateur
Le terminal reste l'application principale, et le navigateur est une seconde fenêtre sur la **même
session en direct**. `w` démarre/arrête la vue web, `o` l'ouvre. **`--lan`** (ou `W`) la sert à votre
réseau local **en HTTPS**, avec une clé d'accès à usage unique et votre approbation pour chaque invité.

### Pilotée par la config, honnête par nature
- Chaque outil est une **entrée YAML** : on en ajoute un sans toucher au code.
- **Détection des imposteurs** : `verify:` repère les programmes homonymes et refuse de lancer le mauvais.

## Le catalogue

**45 outils répartis en 11 catégories**, tous choisis parce qu'ils tournent bien **nativement** sous Windows.

| Catégorie | Outils |
|---|---|
| **Recon** | nmap · rustscan · gobuster · httpx · subfinder · amass · gau |
| **Web** | sqlmap · nuclei · ffuf · feroxbuster · katana · arjun · dirsearch · wfuzz · commix · nikto · ZAP (GUI) |
| **Mots de passe** | hashcat · John the Ripper (jumbo) · hashid · NetExec |
| **Crypto** | openssl · RsaCtfTool · CyberChef (navigateur) |
| **Forensique** | exiftool · strings (Sysinternals) · Sleuth Kit · Volatility 3 |
| **Rétro-ingénierie** | radare2 · upx · jadx · apktool · Detect It Easy · Cutter (GUI) · x64dbg (GUI) · dnSpy (GUI) · Ghidra (GUI) |
| **Pwn** | pwntools · ropper |
| **OSINT** | theHarvester · Sherlock |
| **Stégano** | steghide |
| **Sniffing** | tshark |
| **Wordlists** | SecLists-lite (données) |

> [!IMPORTANT]
> **Le sans-fil, honnêtement :** la capture Wi-Fi en mode moniteur et l'injection de paquets **ne
> fonctionnent pas** sous Windows avec des adaptateurs classiques. Seul le cassage *hors ligne* de
> fichiers déjà capturés est utile ici. Pour du vrai travail sans-fil, utilisez Linux avec un
> adaptateur USB compatible.

## Utilisation

```text
winsploit                 lance le tableau de bord interactif
winsploit list            liste le catalogue avec le statut d'installation
winsploit hashid <h>…     identifie un ou plusieurs hashs (indices hashcat -m / john --format)
winsploit --help, -h      affiche l'aide
winsploit --version, -V   affiche la version
```

### Touches du tableau de bord

| Touche | Action |
|---|---|
| `↑` `↓` | Déplacer la sélection |
| `/` | Filtrer par nom, catégorie ou description |
| `Entrée` | Lancer l'outil sélectionné (modifiez les arguments préremplis, puis `Entrée`) |
| `i` | Installer l'outil (Scoop ou pipx) |
| `r` | Re-détecter ce qui est installé |
| `f` · `n` · `N` | Chercher dans la sortie · occurrence précédente · suivante |
| `c` | Annuler le processus en cours (tout l'arbre) |
| `h` | Identifier un hash (intégré, hors ligne) |
| `a` · `A` | Agent IA : lire la sortie et proposer les étapes · choisir le fournisseur / modèle |
| `p` | Projets : créer / ouvrir un dossier de travail, avec récents |
| `d` | Dossier de travail : là où tournent les outils, comme `cd` |
| `C` | Challenges : créer, changer, définir la cible / le format de flag |
| `F` | Flags capturés dans la sortie |
| `S` | Lancer l'outil dans Windows Sandbox |
| `t` | Apparence : thème, bordures, glyphes (aperçu en direct) |
| `w` · `W` · `o` | Vue web : démarrer / servir le LAN / ouvrir |
| `?` | Afficher toutes les touches |
| `q` | Quitter |

---

📖 **Référence complète** (modèle de sécurité, architecture, ajout d'un outil, recettes, sandbox…) :
voir le **[README en anglais](README.md)**, qui est la version canonique.

## Contribuer

Ajouter un outil est le plus simple : ajoutez une entrée dans [`tools.yaml`](tools.yaml) (et un
manifeste `bucket/` si personne ne l'empaquette), lancez `cargo test` et ouvrez une PR. N'ajoutez que
des outils qui marchent vraiment nativement sous Windows, et documentez les réserves avec `note:`.

## Licence

[MIT](LICENSE) © les contributeurs de Winsploit. Chaque outil fourni ou installé garde sa propre licence.
