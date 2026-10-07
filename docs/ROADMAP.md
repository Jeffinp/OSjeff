# Roadmap

O que existe, o que falta e **por que nessa ordem**. A ordem vem da auditoria
([`audit/RELATORIO.md`](audit/RELATORIO.md)): primeiro o que pode derrubar o sistema,
depois o que limita o que ele consegue fazer, depois polimento.

## Feito

| Área | Entregue |
|---|---|
| Base | Boot BIOS e UEFI, compositor com damage tracking, window manager, 7 apps, scheduler preemptivo, heap com coalescência, OJFS com pastas e lixeira, ATA PIO, NE2000 + ARP/IPv4/ICMP/DHCP, TCP/IP (`smoltcp`) e TLS 1.3, navegador HTML/CSS, runtime WebAssembly (`wasmi`) |
| Robustez | GDT/TSS com IST para #DF e #PF; todas as exceções tratadas; tela de erro e serial em panic/exceção/OOM; **thread secundária que falha morre sozinha**; **páginas de guarda nas pilhas**; IRQ espúria; recusa de framebuffer grande; disco intocado se a leitura falhar; editor não salva buffer truncado |
| Parsers | 9 bugs achados por fuzzing/leitura e corrigidos (rede, disco, HTML/CSS, URL, `chunked`); limites de corpo, profundidade, nós e regras; redirect seguro |
| Scheduler | Estado "bloqueada" (`sched::block/wake/idle`), compositor de 83 → 250 iterações/s em idle, latência tecla→captura de ~11 ms → ~0,4 ms, CPU real no gerenciador de tarefas |
| WebAssembly | *fuel* por chamada, limite de memória, término real do app, tetos nas host functions |
| Desempenho | Tick do relógio 15,6 → 0,2 ms; quadro de tecla 26 → 13 ms; preenchimento 24 bpp 14 → 3 ciclos/px |
| Qualidade | 423 testes (de 189); ~11,7 mil linhas de lógica no `osjeff_core` testadas no host; `unsafe` 100% documentado e imposto pelo lint; CI, `cargo deny`, fuzzing, harness de boot em QEMU |

## Próximos passos, em ordem

Cada item tem um **critério de aceite verificável** (como se prova que acabou).

### 1. Reiniciar e limpar uma thread que morreu
~~Uma thread que falha não derruba a máquina~~ **feito** (S3/S4 do ADR): panic ou
exceção em `fetcher`/`wasmapp` mata só a thread, e as pilhas têm página de guarda. Falta
**reiniciar** a thread (hoje o navegador e o app WASM ficam inutilizáveis até o reboot) e
**liberar** o que ela usava (pilha, alocações, `Store` do `wasmi`, locks).
*Aceite:* matar o `fetcher` e abrir o navegador de novo funciona sem reiniciar o sistema.

### 2. HTTPS de verdade
Trust store, relógio confiável e verificação de cadeia com `embedded-tls`
(`CertVerifier`); sem `RDRAND`, recusar em vez de usar RNG fraco.
*Aceite:* conectar a um servidor com certificado inválido falha; a barra deixa de
dizer "não verificada" apenas para cadeias válidas.

### 3. Rede que funciona fora do QEMU
- ~~O DHCP alimenta o `netstack`~~ **feito** (`NetConfig`): o navegador carregou uma página numa sub-rede `192.168.77.0/24` com gateway e DNS do lease. Falta renovar o lease e usar mais de um DNS.
- Um driver para uma NIC comum (`virtio-net`, depois `e1000` ou `rtl8139`); o NE2000 é ISA e raro.
*Aceite:* o navegador carrega uma página usando uma NIC que existe em hardware real.

### 4. WebAssembly como fronteira de isolamento
*Fuel* retomável (um quadro pesado legítimo, como o carregamento de nível do DOOM,
excede 20 M hoje); escolha de app em tempo de execução (hoje é um por build);
lista de apps; botão de encerrar/reiniciar; WASI sobre o OJFS.
*Aceite:* dois apps WASM abertos ao mesmo tempo, um deles hostil, sem afetar o outro.

### 5. Hardware real
Nenhum teste fora do QEMU. Testar em UEFI real, medir o custo de VRAM (hoje ~1 MB por
tecla, framebuffer sem *write-combining*), dimensionar os buffers pelo framebuffer
(hoje fixos em 1080p), adaptar à resolução.
*Aceite:* boot e desktop num PC real com tela > 1080p; custo de quadro medido lá.

### 6. Mais kernel testado
Continuar migrando lógica pura para o `osjeff_core`: o despacho de entrada
(`desktop/input.rs`, exige trocar chamadas diretas por um enum de comandos), a lógica de
dano do `render.rs`, o parse do cabeçalho do anel do DP8390.
*Aceite:* o kernel perde linhas de decisão a cada PR e o core ganha testes.

### 7. Fuzzing contínuo
Rodar as regressões de `fuzz/regressions/` no CI e campanhas longas (horas) dos três
alvos; fuzz do `smoltcp` e do parser de `.wasm` via harness próprio; modelo do
allocator sob Miri.
*Aceite:* job de CI que falha se uma entrada de regressão voltar a travar.

### 8. Desempenho restante (medido, ainda aberto)
Enviar à VRAM só o que mudou (≈ 1 MB por tecla para ≈ 0,3 KB reais); cache de
glifos (texto 8×8 ≈ 1,9 mil ciclos/caractere); *flush* incremental do OJFS (hoje 99
setores de PIO bloqueiam o compositor por ~45 ms no TCG); alocação O(n) com muitos
buracos.
*Aceite:* cada um com o número antes/depois e screenshot idêntico, como na auditoria.

### 9. Ring 3
Só com gatilho concreto: binários nativos de terceiros, uma fuga comprovada do
`wasmi`, ou vários usuários. Custo estimado: 22–33 dias de um desenvolvedor para o
MVP de um processo, 50–75 com os apps portados ([ADR](audit/adr-isolamento.md)).

## Fora do escopo (de propósito)

SMP/APIC, prioridades no scheduler, USB, som, sistema de arquivos com journaling,
trocar o bootloader. Nenhum deles resolve um problema medido hoje.
