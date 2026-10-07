# Imagens no OSjeff: `inflate`, `deflate`, `image`, `bmp`, `png`, `ppm`

Bibliotecas puras em `osjeff_core` (`no_std` + `alloc`, `forbid(unsafe_code)`,
sem dependências novas, testadas no host) que servem de base para o visualizador
de imagens, ícones e wallpapers. O kernel não foi alterado.

```
inflate (DEFLATE + zlib + CRC-32/Adler-32)     deflate (codificador: stored / LZ77 + Huffman fixo)
        \                                         /
         png (decodifica; codifica) ------------+            bmp (decodifica; codifica 24/32)    ppm (P3/P6)
                \                                                  |                               /
                 +----------------------- image::{Image, decode, detect, encode} ----------------+
```

## Formato de pixel e operações (`image`)

`Image { largura, altura, Vec<u32> }`, pixel `0xAARRGGBB` (a mesma ordem do
framebuffer), alfa **reto** (não pré-multiplicado), linhas de cima para baixo.
Não existe imagem de tamanho zero.

| Operação | Observação |
|---|---|
| `crop(x, y, w, h)` | erro se o retângulo sai da imagem |
| `resize_nearest` / `resize_bilinear` / `resize_box` / `resize(w, h, Filter)` | `Filter::{Nearest, Bilinear, Box, Auto}`; `Auto` = caixa ao reduzir, bilinear ao ampliar |
| `fit_dims` / `fit(caixa_w, caixa_h, ampliar, filtro)` | cabe na caixa mantendo a proporção; só amplia se `ampliar` |
| `rotate90`, `rotate270`, `rotate180`, `flip_horizontal`, `flip_vertical` | as três últimas são in-place |
| `flatten(fundo)`, `blit_over(&src, x, y)`, `over(src, dst)` | composição "source over", recorte nas bordas |
| `to_rgba`, `from_rgba`, `from_pixels`, `row`, `get`, `set` | acesso sem pânico (fora do limite = `None`/`false`/fatia vazia) |

**Aritmética inteira, sem `f32`/`f64`** (o kernel é soft-float):

* bilinear: posições com 8 bits fracionários (1/256 de pixel), pesos 2-D somando
  exatamente 65536 (imagem constante continua exatamente constante);
* caixa: pesos de área exatos em inteiros, passada horizontal com 8 bits extras,
  linhas de origem processadas em fluxo (memória extra `O(largura)`);
* ambos interpolam em alfa pré-multiplicado (pixel transparente não "mancha" a vizinhança).

## Limites de segurança

* `MAX_PIXELS` = 16 Mpx (64 MiB de pixels). A checagem é feita nas **dimensões**,
  com aritmética sem estouro, antes de qualquer alocação (`pixel_count`). Toda
  alocação usa `try_reserve` (`ImageError::OutOfMemory` em vez de abortar).
* `inflate`: `max_output` é **obrigatório** e exato; a saída só cresce na medida em
  que bytes reais são produzidos (uma entrada pequena não reserva `max_output`).
  Memória fixa: janela de 32 KiB + 2 tabelas de Huffman. Nunca entra em pânico.
* PNG: o tamanho descomprimido exato vem do cabeçalho e é passado como
  `max_output`; sobra/falta de dados são erros distintos. Cabeçalho que promete mais
  que a razão máxima do deflate (~1032:1) permite é recusado **antes** de alocar a
  imagem. Linhas são inflacionadas e "desfiltradas" uma a uma, direto na imagem
  (o fluxo descomprimido nunca fica inteiro na memória). CRC de todos os chunks é
  verificado; ordem de chunks, chunk crítico desconhecido, índice de paleta e
  tamanhos de `PLTE`/`tRNS` são validados.
* BMP: dimensões e bytes realmente presentes são conferidos antes de alocar.
* PPM: idem (um número `P3` precisa de ao menos 2 bytes).

## API resumida

```rust
use osjeff_core::image::{self, Filter, Format, Image};

let img: Image = image::decode(&bytes)?;           // PNG, BMP ou PPM, por assinatura
let tela = img.fit(largura_janela, altura_janela, false, Filter::Auto)?;
let miniatura = img.fit(96, 96, false, Filter::Box)?;
let png: Vec<u8> = image::encode(&captura, Format::Png)?;   // "salvar captura"
```

Módulos de baixo nível: `png::{decode, read_header, encode, encode_rgba}`,
`bmp::{decode, encode_24, encode_32}`, `ppm::{decode, encode_p6}`,
`inflate::{inflate, zlib_decompress, Inflater, crc32, adler32}`,
`deflate::{deflate_stored, deflate_fixed, zlib_stored, zlib_compress}`.
Erros são enums sem dados (`Copy`, `Display`): `DecodeError` embrulha
`PngError`/`BmpError`/`PpmError`, que embrulham `InflateError`/`ImageError`.

## Como o visualizador deve usar

1. Ler o arquivo para um `Vec<u8>` (o FS já limita o tamanho) e chamar
   `image::decode`. O erro tipado vira a mensagem da barra de status
   (`DecodeError` implementa `Display`).
2. Ajustar à janela com `fit(..., Filter::Auto)` (reduz com caixa, amplia com
   bilinear). Para pixel art/ícones pequenos use `Filter::Nearest`. Recalcule só
   quando a janela mudar de tamanho; guarde o resultado.
3. Rotação/espelhamento: `rotate90`/`rotate270`/`flip_*` sobre a imagem original
   (não sobre a ampliada) e depois `fit` de novo.
4. Desenhar: `flatten(cor_do_fundo_da_janela)` deixa a imagem opaca; os pixels
   (`pixels()`) já estão em `0xAARRGGBB`, prontos para o framebuffer. Para ícones
   com transparência use `blit_over` sobre o fundo.
5. Wallpaper "preencher": `resize` até cobrir a tela e `crop` central
   (as duas operações já existem; `fit` é o modo "caber").
6. Miniaturas de gerenciador de arquivos: `fit(n, n, false, Filter::Box)`.
7. Salvar captura: `image::encode(&img, Format::Png)` (RGB8 se opaca, RGBA8
   senão; filtro adaptativo + LZ77 com Huffman fixo) ou `Format::Bmp`
   (24 bits se opaca, 32 com alfa).

## Comportamentos que valem registrar

* PNG: gamma/`iCCP`/`sRGB` são ignorados; 16 bits viram 8 por `(v + 128) / 257`;
  APNG mostra só o quadro padrão; chunk **crítico** desconhecido é erro, ancilar
  desconhecido é pulado; bytes depois do `IEND` e depois do fluxo zlib são ignorados.
* BMP: índice de paleta fora do intervalo = preto opaco; 32 bpp com todos os bytes de
  alfa zero é tratado como opaco; pixels pulados por RLE ficam transparentes.
* O codificador de PNG usa só Huffman fixo: o arquivo sai maior que o do zlib,
  mas é sempre válido (conferido contra ImageMagick/libpng).
* Não há JPEG, GIF nem WebP.

## Desempenho (host, release, melhor de 15; imagem 1024x768 RGBA = 3 MiB)

Medido com `cargo run --release -p osjeff_core --example image_bench`. O host tem
SSE e vetoriza; o kernel é soft-float/sem SSE, então espere números maiores lá
(use as **razões**, não os milissegundos absolutos).

| Operação | Tempo | Razão |
|---|---|---|
| decodificar PNG (fluxo do nosso codificador) | 18,7 ms | 168 MB/s de pixels |
| decodificar PNG real do libpng (1,6 MiB) | 25,3 ms | 124 MB/s; o `inflate` é ~90% do tempo, desfiltrar + converter é o resto |
| `inflate` puro, mesma carga | 18,7 ms | 168 MB/s de saída |
| bilinear 1024x768 -> 512x384 | 1,8 ms | 0,07x o tempo do decode |
| bilinear 1024x768 -> 1280x960 | 11,6 ms (9,5 ms se opaca) | 0,46x o decode |
| caixa 1024x768 -> 320x240 (miniatura) | 7,2 ms | 0,29x o decode |
| `rotate90`, `flatten` | 2,8 ms, 1,2 ms | |
| CRC-32 (fatias de 8), Adler-32 | 1,7 GB/s, 2,9 GB/s | verificar o PNG custa ~0,1% do decode |
| codificar PNG (LZ77 + Huffman fixo) | 154 ms | 20 MB/s; saída 45% do bruto neste quadro com ruído |

Alocações: o decodificador de PNG aloca a imagem, duas linhas, o arquivo de IDAT
concatenado só quando há mais de um chunk, a janela de 32 KiB e duas tabelas de Huffman
(~5 KiB). O redimensionamento por caixa usa `O(largura)` extra; o bilinear, uma tabela
de `largura` entradas. Não há `unsafe`: os índices do anel de 32 KiB são mascarados e
os laços de pixel usam `as_chunks`/`zip`, o que deixa o compilador tirar quase todas as
checagens de limite.

## Verificação

* Testes unitários em `osjeff_core` (`cargo test -p osjeff_core image:: png:: bmp::
  ppm:: inflate:: deflate::`). Vetores reais embutidos como hex: fluxos zlib do
  CPython, arquivos BMP/PNG/PPM do ImageMagick/libpng (com os pixels brutos do
  próprio `convert ... rgba:-` como esperado), mais arquivos montados em Python por
  um implementação independente de filtros/Adam7. Teste automático nunca depende de
  Python nem de ImageMagick.
* Fuzz: `fuzz/fuzz_targets/image_decode.rs` (ver `docs/TESTING.md`). Além de
  "não pode travar", verifica que `png::encode`/`bmp::encode_*` fazem ida e volta
  exata com o que o decodificador aceitou. 10 minutos: ~300 mil execuções,
  ~500/s, 3087 arestas de cobertura, nenhum crash (cobertura de linhas ao
  reproduzir o corpus: `bmp` 95%, `png` 94%, `ppm` 89%, `image` 84%, `inflate` 83%,
  `deflate` 82%). Entradas que o fuzzer achar ruins vão para
  `fuzz/regressions/image_decode/`.
* Medição no host: `cargo run --release -p osjeff_core --example image_bench`.
