# Marca Kitsune

A marca é uma **raposa geométrica de frente**, feita de facetas planas. Ela existe como dado
vetorial (12 polígonos numa grade 128 x 128) em `kitsune_core/src/brand.rs`; a interface a
desenha de lá e os arquivos desta pasta são gerados dos mesmos polígonos, então nada sai do
compasso. O nome e a pronúncia estão em [`NAMING.md`](NAMING.md).

| Arquivo | O que é |
|---|---|
| [`kitsune-tile.svg`](kitsune-tile.svg) | o ícone: a raposa no quadrado arredondado índigo (raio de 22 %) |
| [`kitsune-mono.svg`](kitsune-mono.svg) | a silhueta de uma cor, com olhos e nariz vazados: favicon, glifo do painel, uso monocromático ou simbólico |
| [`kitsune-halo.svg`](kitsune-halo.svg) | a raposa na frente de uma coroa de sete caudas (vinheta de abertura, Sobre) |
| [`kitsune-halo-9.svg`](kitsune-halo-9.svg) | a mesma coroa com as nove caudas (ilustração; não é usada na interface) |
| [`sizes.png`](sizes.png) | o ícone em 16, 24, 32, 48, 64 e 128 px, em fundo escuro e claro, como a interface o desenha |
| [`social-preview.png`](social-preview.png) | imagem de 1280 x 640 para a prévia social do repositório |

Para regerar tudo depois de mexer em `brand.rs`:

```bash
cargo run -p kitsune_core --example brand_svg -- docs/brand --ppm
cd docs/brand && convert sizes.ppm sizes.png && convert social-preview.ppm social-preview.png && rm *.ppm
```

O teste `the_32px_tile_matches_the_golden_hash` falha quando o desenho do tile de 32 px muda
sem querer; se a mudança é proposital, o comentário de `GOLDEN_TILE_32` diz como olhar o
resultado e atualizar o hash.

## Paleta

| Papel | Cor | Onde |
|---|---|---|
| Índigo (tile) | `#2A2A5C` | fundo do ícone |
| Laranja claro | `#FF7A33` | metade esquerda do rosto |
| Ferrugem | `#C94F1C` | metade direita do rosto, bochechas, caudas |
| Laranja da orelha | `#F76B2A` | face externa das orelhas |
| Ferrugem escura | `#B7371A` | face interna das orelhas, caudas alternadas |
| Creme | `#FFF3E6` | focinho e bochechas em V, pontas das caudas |
| Noite | `#1A1A3A` | olhos e nariz |

O acento padrão da interface (`#5B5CF6`, índigo) é outra coisa: ele é do usuário e muda nos
Ajustes; as cores da marca não mudam com ele.

## Tamanhos e ajustes finos

A marca é redesenhada para cada tamanho, não só reduzida: abaixo de 48 px os olhos e o nariz
crescem (210 % em 16 px, 165 % em 24 px, 135 % em 32 px) para manterem cerca de dois pixels, e
em 16 px as facetas escuras das orelhas se fundem às claras. A tabela está em `brand::hint`.

- **Tamanho mínimo:** 16 px para o ícone colorido e para a silhueta; 64 px para a versão com
  caudas (abaixo disso a coroa vira uma mancha). Em impressão, 8 mm.
- **Área de respiro:** pelo menos 1/4 da largura do ícone (32 unidades da grade) livres em todos
  os lados, sem texto, borda nem outra imagem encostada. A silhueta e a versão com caudas não têm
  tile: a respiração conta a partir da ponta das orelhas e das caudas.
- **Com o nome:** "Kitsune" em Inter Semibold, à direita ou abaixo da marca, com a altura das
  maiúsculas igual a 1/3 da altura da raposa; veja `social-preview.png`.

## O que não fazer

- **Nunca enrolar a raposa num globo, num círculo ou numa órbita**: ela identifica o sistema, não
  um app. O navegador do sistema se chama só Navegador.
- Não usar a palavra "Fox" no nome de nada (veja `NAMING.md`).
- Não recolorir a versão colorida; para uma cor só, use a silhueta (`kitsune-mono.svg`) na cor
  do contexto, em contraste suficiente com o fundo.
- Não esticar, girar, inclinar, espelhar (a marca é simétrica, mas o rosto tem um lado claro e
  um escuro), contornar nem aplicar sombras, brilhos ou degradês.
- Não mudar o raio do tile (22 %) nem apará-lo num círculo.
- Não redesenhar as caudas nem mudar o número delas na vinheta (sete) sem passar por
  `brand::HALO_TAILS`, que explica por quê.
- A marca não pode ser usada de um jeito que sugira apoio ou autoria de terceiros.

O símbolo ™ só aparece no cabeçalho do `README.md` e do `README.en.md`, enquanto o registro no
INPI não sai.
