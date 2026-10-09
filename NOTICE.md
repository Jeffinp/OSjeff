# Avisos e componentes de terceiros

O código do OSjeff é de Jeferson Reis Almeida e está sob a [PolyForm Strict 1.0.0](LICENSE) (veja
também [LICENSE-CONTRIBUTORS.md](LICENSE-CONTRIBUTORS.md)). Os componentes abaixo **não** são do
Titular e mantêm as licenças originais, que são permissivas e permitem a inclusão no sistema.

## Fontes embutidas (SIL Open Font License 1.1)

| Fonte | Arquivo de licença |
|---|---|
| Inter (Regular, Medium, SemiBold; subconjunto) — © The Inter Project Authors | [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt) |
| JetBrains Mono (Regular; subconjunto) — © JetBrains s.r.o. | [`assets/fonts/OFL-JetBrainsMono.txt`](assets/fonts/OFL-JetBrainsMono.txt) |

A OFL exige manter o aviso de copyright e a licença junto da fonte, o que este repositório faz.
Os subconjuntos não usam o nome reservado das fontes originais para versões modificadas além do
que a licença permite.

## Bibliotecas Rust

O sistema depende de bibliotecas de código aberto (por exemplo `bootloader`, `x86_64`,
`smoltcp`, `embedded-tls`, `rustls-webpki`, `wasmi`, `spin`, `miniz_oxide`, entre outras), todas com
licenças permissivas (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, MPL-2.0 nas condições do arquivo
`deny.toml`). A lista completa e as versões estão em `Cargo.lock`; o comando
`cargo deny check licenses` confere que nenhuma dependência usa licença fora dessa política. Os
avisos de copyright de cada biblioteca ficam no código-fonte dela, distribuído junto com o crate.

## Ferramentas e dados

- Lojas de certificados raiz embutidas (`kitsune_core/data/trust-store.bin`): derivadas da lista
  pública de autoridades certificadoras da Mozilla (MPL-2.0); veja `tools/gen-trust-store.sh`.
