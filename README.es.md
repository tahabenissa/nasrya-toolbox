<div align="center">

[English](README.md) · [Français](README.fr.md) · **Español** · [中文](README.zh.md) · [العربية](README.ar.md) · [Русский](README.ru.md)

<a href="https://tahabenissa.github.io/winsploit/">
  <img src=".github/banner.svg" alt="Winsploit" width="720">
</a>

### La caja de herramientas de seguridad nativa de Windows, en tu terminal.

**Encuentra, instala y ejecuta 45 herramientas de pentest y CTF desde un panel controlado por teclado.**<br>
Sin WSL. Sin VM. Sin dudas sobre qué funciona de verdad en Windows.

<br>

<a href="https://tahabenissa.github.io/winsploit/"><img src="https://img.shields.io/badge/Sitio%20web-89c2d2?style=for-the-badge&logo=googlechrome&logoColor=0c191d" alt="Sitio web" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/docs/"><img src="https://img.shields.io/badge/Docs-232831?style=for-the-badge&logo=readthedocs&logoColor=89c2d2" alt="Docs" height="36"></a>&nbsp;
<a href="https://tahabenissa.github.io/winsploit/demo/"><img src="https://img.shields.io/badge/Demo-232831?style=for-the-badge&logo=gnubash&logoColor=89c2d2" alt="Demo" height="36"></a>

<br>

[![CI](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml/badge.svg)](https://github.com/tahabenissa/winsploit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/tahabenissa/winsploit?include_prereleases&sort=semver)](https://github.com/tahabenissa/winsploit/releases)
![platform](https://img.shields.io/badge/plataforma-Windows%2010%20%7C%2011-0078D6)
![tools](https://img.shields.io/badge/cat%C3%A1logo-45%20herramientas-8A2BE2)
![license](https://img.shields.io/badge/licencia-MIT-green)

</div>

---

> [!WARNING]
> **Solo uso autorizado.** Winsploit es para pruebas de penetración autorizadas, investigación de
> seguridad, CTF y aprendizaje. Apúntalo únicamente a sistemas que te pertenezcan o para los que
> tengas **permiso explícito por escrito**. El escaneo o ataque no autorizado es ilegal en la mayoría
> de jurisdicciones, y es tu responsabilidad mantenerte dentro de tus reglas de enfrentamiento.

---

## ¿Por qué Winsploit?

Kali es el hogar del tooling de seguridad, pero mucha gente trabaja en **Windows**, y las opciones
existentes te obligan a transigir:

| Opción | Lo bueno | Lo que falta |
|---|---|---|
| **Commando VM / FLARE-VM** | Excelentes, completas | *Provisionadores de VM* de un solo uso, no algo del día a día |
| **PentestBox** | La idea correcta: nativo y portable | Abandonado desde ~2016, licencia poco clara |
| **UniGetUI** | Gran gestor de paquetes | Sin curación de seguridad, sin flujo de «ejecutar la herramienta» |
| **Winsploit** | Curado, nativo, instala **y** ejecuta, MIT, en desarrollo activo | Alpha |

No existe un lanzador nativo de Windows mantenido y con licencia clara que a la vez *instale* y
*ejecute* herramientas de seguridad desde una sola interfaz. Winsploit llena ese vacío, y es honesto
sobre lo que de verdad funciona en Windows y lo que necesita Linux.

## Inicio rápido

**Lo más fácil (sin admin):** descarga `winsploit-<versión>-setup.exe` desde la página de
[Releases](https://github.com/tahabenissa/winsploit/releases). El instalador lo añade a tu PATH,
puede configurar Scoop, git, los buckets y pipx, y te deja marcar paquetes de herramientas. Abre una
**nueva** terminal y escribe `winsploit`. El binario no está firmado (SmartScreen: «Más información»
y luego «Ejecutar de todas formas»); verifícalo con `SHA256SUMS`.

**O compilar desde el código fuente:**

**Necesitas:** Windows 10/11, [Scoop](https://scoop.sh) y [Rust 1.87+](https://rustup.rs).

```powershell
# 1. Instalar Scoop (omitir si ya lo tienes)
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

# 2. Compilar winsploit
git clone https://github.com/tahabenissa/winsploit
cd winsploit
cargo build --release

# 3. Convertir `winsploit` en comando (añade target\release a tu PATH, sin admin)
.\target\release\winsploit setup --path --shortcut

# 4. Abrir una nueva terminal, ir a donde trabajas y lanzarlo
cd C:\ctf\una-box
winsploit
```

Elige una herramienta con `↑/↓` (o `/` para filtrar), pulsa `i` para instalarla y `Enter` para
ejecutarla. Pulsa `?` en cualquier momento para ver todas las teclas.

## Características

### Un panel pensado para el teclado
- **Explora por categoría** con estado de instalación en vivo: `●` instalada, `○` ausente, `◐` conflicto.
- **Filtro instantáneo (`/`)** por nombre, categoría o descripción.
- **Un editor de línea de verdad** para los argumentos, con **historial** por herramienta (`↑/↓`).
- **Ayuda de teclas (`?`)**.

### Salida en vivo e interactiva
- stdout/stderr **se transmiten al panel** en tiempo real, con color.
- **Habla con las herramientas en ejecución.** Cada proceso tiene su propio canal de entrada (`Ctrl+D` = fin de entrada).
- **Desplazamiento** (`PgUp/PgDn/Home/End`) y **búsqueda en la salida** (`f`, `n`, `N`).
- **`c` cancela** todo el árbol de procesos, no solo el padre.

### Instalaciones de una tecla, bien hechas
- **Scoop** para herramientas nativas, con **buckets añadidos automáticamente** cuando se necesitan.
- **pipx** para herramientas Python (cada una en su venv; instala Python y pipx si faltan).
- **El bucket Scoop propio de Winsploit** para lo que nadie empaqueta, con **hash SHA-256**.
- La detección corre **fuera del hilo de la UI**: el panel nunca se congela.

### Hecho para CTF
- **Espacios de reto (`C`)**: una carpeta por reto con su objetivo, formato de flag, archivos, notas, logs y flags.
- **Recetas**: `Enter` en nmap, ffuf, john… ofrece comandos listos con `{target}`, `{file}`, `{out}`, `{wordlist}` rellenados.
- **Capturador de flags**: `picoCTF{…}`, `HTB{…}`, `flag{…}` resaltados, anunciados, copiados y guardados.

### Un agente de IA en el bucle
- Pulsa **`a`**: una IA lee la salida del terminal y sugiere los siguientes pasos como **comandos de
  herramientas del catálogo que confirmas antes de ejecutarlos**. El agente nunca ejecuta un shell.
- **`A`** elige el proveedor: **CLI de Claude** (reutiliza tu sesión, sin clave), o
  **Gemini / DeepSeek / OpenAI**, o un modelo **Ollama** local — con un modelo configurable por
  proveedor. Las claves se leen del entorno.

### La misma sesión en tu navegador
La terminal sigue siendo la app principal, y el navegador es una segunda ventana a la **misma sesión
en vivo**. `w` inicia/detiene la vista web, `o` la abre. **`--lan`** (o `W`) la sirve a tu red local
**por HTTPS**, con una clave de acceso de un solo uso y tu aprobación para cada invitado.

### Guiada por config, honesta por diseño
- Cada herramienta es una **entrada YAML**: añades una sin tocar código.
- **Detección de impostores**: `verify:` detecta programas con el mismo nombre y se niega a lanzar el equivocado.

## El catálogo

**45 herramientas en 11 categorías**, todas elegidas porque funcionan bien **de forma nativa** en Windows.

| Categoría | Herramientas |
|---|---|
| **Recon** | nmap · rustscan · gobuster · httpx · subfinder · amass · gau |
| **Web** | sqlmap · nuclei · ffuf · feroxbuster · katana · arjun · dirsearch · wfuzz · commix · nikto · ZAP (GUI) |
| **Contraseñas** | hashcat · John the Ripper (jumbo) · hashid · NetExec |
| **Cripto** | openssl · RsaCtfTool · CyberChef (navegador) |
| **Forense** | exiftool · strings (Sysinternals) · Sleuth Kit · Volatility 3 |
| **Reversing** | radare2 · upx · jadx · apktool · Detect It Easy · Cutter (GUI) · x64dbg (GUI) · dnSpy (GUI) · Ghidra (GUI) |
| **Pwn** | pwntools · ropper |
| **OSINT** | theHarvester · Sherlock |
| **Estego** | steghide |
| **Sniffing** | tshark |
| **Wordlists** | SecLists-lite (datos) |

> [!IMPORTANT]
> **El wireless, con honestidad:** la captura Wi-Fi en modo monitor y la inyección de paquetes **no
> funcionan** en Windows con adaptadores normales. Aquí solo es útil el crackeo *offline* de archivos
> ya capturados. Para trabajo wireless real, usa Linux con un adaptador USB compatible.

## Uso

```text
winsploit                 lanza el panel interactivo
winsploit list            lista el catálogo con estado de instalación
winsploit hashid <h>…     identifica uno o más hashes (pistas hashcat -m / john --format)
winsploit --help, -h      muestra la ayuda
winsploit --version, -V   muestra la versión
```

### Teclas del panel

| Tecla | Acción |
|---|---|
| `↑` `↓` | Mover la selección |
| `/` | Filtrar por nombre, categoría o descripción |
| `Enter` | Ejecutar la herramienta seleccionada (edita los argumentos, luego `Enter`) |
| `i` | Instalar la herramienta (Scoop o pipx) |
| `r` | Re-detectar lo instalado |
| `f` · `n` · `N` | Buscar en la salida · coincidencia anterior · siguiente |
| `c` | Cancelar el proceso en curso (todo el árbol) |
| `h` | Identificar un hash (integrado, sin conexión) |
| `a` · `A` | Agente IA: leer la salida y sugerir pasos · elegir proveedor / modelo |
| `p` | Proyectos: crear / abrir una carpeta de trabajo, con recientes |
| `d` | Carpeta de trabajo: donde corren las herramientas, como `cd` |
| `C` | Retos: crear, cambiar, definir objetivo / formato de flag |
| `F` | Flags capturados en la salida |
| `S` | Ejecutar la herramienta en Windows Sandbox |
| `t` | Apariencia: tema, bordes, glifos (vista previa en vivo) |
| `w` · `W` · `o` | Vista web: iniciar / servir la LAN / abrir |
| `?` | Mostrar todas las teclas |
| `q` | Salir |

---

📖 **Referencia completa** (modelo de seguridad, arquitectura, añadir una herramienta, recetas,
sandbox…): consulta el **[README en inglés](README.md)**, que es la versión canónica.

## Contribuir

Añadir herramientas es lo más fácil para empezar: añade una entrada en [`tools.yaml`](tools.yaml) (y
un manifiesto `bucket/` si nadie la empaqueta), ejecuta `cargo test` y abre una PR. Añade solo
herramientas que funcionen de verdad de forma nativa en Windows, y documenta las salvedades con `note:`.

## Licencia

[MIT](LICENSE) © los contribuidores de Winsploit. Cada herramienta incluida o instalada conserva su propia licencia.
