# Auditoria de desempenho, segurança e boas práticas

Auditoria feita em outubro de 2026 sobre o estado `fc89615` da `master`. Os relatórios
descrevem o que foi **encontrado**; vários achados já foram corrigidos depois, e o
[`RELATORIO.md`](RELATORIO.md) mantém o status de cada um (coluna *Status*).
Comece por ele.

| Documento | O que contém |
|---|---|
| [`RELATORIO.md`](RELATORIO.md) | Resumo, tabela única priorizada, as 10 primeiras ações, o que não vale a pena fazer, afirmações falsas do README, antes/depois |
| [`00-mapa.md`](00-mapa.md) | Boot, layout de memória, privilégio, inventário de `unsafe` e de estáticos, `wasm-apps/` |
| [`01-memoria-unsafe.md`](01-memoria-unsafe.md) | Allocator, `RacyCell`, deadlock de IRQ, `asm!`, catálogo de `unsafe` |
| [`02-interrupcoes-scheduler.md`](02-interrupcoes-scheduler.md) | IST/#DF, troca de contexto, pilhas e canário, PIC, scheduler |
| [`03-superficie-ataque.md`](03-superficie-ataque.md) | Rede, disco, parsers; fuzzing (alvos, números, crashes) |
| [`04-desempenho.md`](04-desempenho.md) | Onde o tempo é gasto, com medições e método |
| [`05-qualidade-rust.md`](05-qualidade-rust.md) | Cobertura real, clippy, dependências, tipos, CI proposto |
| [`adr-isolamento.md`](adr-isolamento.md) | ADR: ring 0 único, WebAssembly vs ring 3, plano incremental |
| [`patches/`](patches/) | Patches medidos mas não aplicados |

**Como ler os níveis de prova.** *PROVADO* = reproduzido (teste, QEMU, execução) ou
trecho conclusivo; *SUPOSIÇÃO* = raciocínio sem reprodução. Os relatórios 00–05 foram
escritos por revisores independentes e depois desafiados pelo revisor do
`RELATORIO.md`, que rebaixou ou descartou o que não tinha prova.

**Limite honesto.** Tudo foi medido em QEMU com TCG (sem KVM). Valores absolutos de
tempo não valem para hardware real; proporções valem. Nada foi testado em hardware.
