# O nome: Kitsune

**Kitsune** (狐, "raposa" em japonês) é, no folclore japonês, a raposa que ganha uma cauda a cada
século de vida e chega a nove: inteligente, ágil, protetora. O sistema herdou a ideia: pequeno,
rápido e esperto. Antes de 2026-10 o projeto se chamava OSjeff (nome do autor); o nome antigo
só aparece onde a história importa (CHANGELOG antigo, relatórios de `docs/audit/`) e nos pontos
de compatibilidade listados abaixo.

## Pronúncia

| Idioma | Como se diz |
|---|---|
| Japonês / inglês | KEET-soo-neh (`/kitsɯne/`) |
| Português do Brasil | "quit-su-nê" |

## Regras de uso do nome

- Escreva **Kitsune**, com K maiúsculo e o resto em minúsculas. Em texto corrido e em código o nome é
  sempre uma palavra só: nada de "KitSune", "Kit-Sune" nem "KITSUNE" fora de títulos em caixa alta.
- **Sem "Fox" no nome.** Nada de "Kitsune Fox", "FoxOS" ou derivados: a raposa é o mascote, não faz parte
  do nome, e "Fox" lembra produtos de terceiros. Nomes de componentes seguem a mesma regra.
- Nome de pacote e de arquivo em minúsculas: `kitsune_core`, `kitsune-bios.img`, `/etc/kitsune.conf`,
  `kitsune://sobre`, nome de máquina `kitsune`.
- A marca é a raposa **sozinha** (veja `README.md` desta pasta); ela nunca é estilizada como logotipo de
  navegador. O navegador do sistema se chama só "Navegador" / "Browser".

## Compatibilidade com o nome antigo

Mantidos de propósito, para que discos, apps e atalhos antigos continuem funcionando:

| Item | Antes | Agora | O que continua valendo |
|---|---|---|---|
| Esquema das páginas internas | `osjeff://` | `kitsune://` | o navegador aceita o antigo e mostra o novo |
| Arquivo de configuração | `/etc/osjeff.conf` | `/etc/kitsune.conf` | o antigo é lido se o novo não existe; some no primeiro salvamento |
| Seções do pacote WASM | `osjeff.manifest`, `osjeff.icon` | `kitsune.manifest`, `kitsune.icon` | as duas grafias são aceitas |
| Módulo de importação dos apps | `osj` | `osj` (sem mudança) | abreviação histórica da ABI, mantida para não quebrar apps já compilados |
| Formato em disco | OJFS (`OJF2`, `OJF3`) | OJFS (sem mudança) | "Kitsune FS (OJFS)" na documentação; uma revisão futura do formato pode renomeá-lo |

## Marca registrada

O titular, Jeferson Reis Almeida, pretende registrar o nome e a marca no INPI. Até lá o símbolo ™ só
aparece no cabeçalho do README; o nome não é usado com ® em lugar nenhum.
