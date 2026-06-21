# Boot do OSjeff em hardware real (pendrive UEFI)

O OSjeff já gera uma imagem **UEFI** booteável. Em hardware real ele desenha
direto no framebuffer da GPU (a Radeon integrada do Ryzen, no seu caso) — sem a
camada de upload por software do QEMU —, então roda **muito mais liso** do que no
emulador. Este guia mostra como gravar e bootar, e lista as limitações reais.

> ⚠️ Projeto de portfólio / hobby OS. Boota em UEFI, mas **não é** um sistema de
> uso diário. Leia as limitações antes.

## 1. Gerar e exportar a imagem

No Windows, na pasta do projeto:

```powershell
.\run.ps1 -Usb
```

Isso compila o release e copia `osjeff-uefi.img` para a raiz do projeto. (Não
abre o QEMU.)

## 2. Gravar no pendrive

A imagem é um **disco cru** (GPT + partição EFI com `EFI/BOOT/BOOTX64.EFI`).
Grave-a **crua** num pendrive — isso **apaga o pendrive inteiro**:

- **Rufus**: selecione `osjeff-uefi.img`, escolha o modo **"DD Image"**, grave.
- **balenaEtcher**: *Flash from file* → `osjeff-uefi.img` → escolha o pendrive.
- **Linux/WSL**: `sudo dd if=osjeff-uefi.img of=/dev/sdX bs=4M conv=fsync` (com
  `/dev/sdX` = o pendrive certo — confira duas vezes, `dd` no disco errado destrói dados).

## 3. Bootar

No PC alvo, entre no firmware (Del/F2/F10/F12 no POST) e:

1. **Secure Boot: DESLIGADO.** O `BOOTX64.EFI` do bootloader não é assinado; com
   Secure Boot ligado o firmware recusa.
2. **Modo UEFI** (não Legacy/CSM). A imagem é UEFI.
3. Dê boot pelo pendrive (boot menu, geralmente F12/F11/F8).

Deve aparecer a splash do OSjeff e cair no desktop.

## 4. Limitações reais (honestas)

Estas valem **só no hardware** — no QEMU tudo funciona porque o emulador provê os
dispositivos legados.

| Área | O que acontece no metal | Por quê |
|---|---|---|
| **Teclado/mouse** | Pode **não funcionar** em notebooks modernos | Driver é **PS/2 (i8042)**. Muitos notebooks só têm USB HID (sem driver nosso) e não emulam PS/2. Desktops geralmente mantêm emulação legada e funcionam. |
| **Resolução > 1080p** | Tela pode sair **corrompida** | Os buffers de composição são fixos em 1920×1080×4. Um painel 1440p/4K dá framebuffer maior que o buffer. Use saída 1080p (monitor externo) por enquanto. |
| **Rede** | Sem rede | A NIC é **NE2000 ISA** (existe no QEMU, não no seu PC). O sistema boota sem rede normalmente. |
| **Disco/persistência** | Filesystem só em RAM (não persiste) | O FS usava um 2º disco IDE do QEMU. No metal, controladoras AHCI/NVMe não respondem nas portas IDE legadas → cai para FS em RAM. Não escreve no seu HD. |
| **Tela congelada** | Trava com a tela parada | Exceções de CPU (#GP/#PF) param com `hlt` de propósito, em vez de reiniciar em silêncio — bug fica visível. |

## 5. Próximos passos para um boot "de verdade" no metal

Itens conhecidos para hardware real virar cidadão de primeira classe (roadmap):

- **Buffers dinâmicos**: dimensionar os buffers de composição pelo framebuffer
  real do GOP (em vez do teto fixo 1920×1080), para suportar 1440p/4K.
- **Driver USB HID** (teclado/mouse) — hoje só PS/2.
- **AHCI/SATA** para persistência real no metal.

> A maioria deles é grande. Para uma demo de portfólio, bootar num **desktop com
> teclado USB+legado e saída 1080p** já mostra o sistema rodando em máquina real.
