/* Tiny C smoke test for the Rust libzopflipng.a C ABI.
 * Does not reference ZopfliCompress / ZopfliInitOptions.
 *
 * cc -O2 -I rust/zopflipng-ffi/include rust/zopflipng-ffi/c_smoke.c \
 *    rust/zopflipng-ffi/target/release/libzopflipng.a -lpthread -ldl -lm \
 *    -o /tmp/zopflipng_smoke
 */

#include "zopflipng_lib.h"

#include <stdio.h>
#include <stdlib.h>
#include <stddef.h>

int main(int argc, char **argv) {
  FILE *f;
  long n;
  unsigned char *in;
  unsigned char *out = NULL;
  size_t outn = 0;
  int rc;
  CZopfliPNGOptions opt;

  printf("sizeof(CZopfliPNGOptions)=%zu\n", sizeof(CZopfliPNGOptions));
  printf("offset lossy_transparent=%zu\n",
         offsetof(CZopfliPNGOptions, lossy_transparent));
  printf("offset filter_strategies=%zu\n",
         offsetof(CZopfliPNGOptions, filter_strategies));
  printf("offset keepchunks=%zu\n", offsetof(CZopfliPNGOptions, keepchunks));
  printf("offset num_iterations=%zu\n",
         offsetof(CZopfliPNGOptions, num_iterations));
  printf("offset block_split_strategy=%zu\n",
         offsetof(CZopfliPNGOptions, block_split_strategy));

  CZopfliPNGSetDefaults(&opt);
  if (opt.num_iterations != 15 || opt.num_iterations_large != 5 ||
      opt.auto_filter_strategy == 0 || opt.use_zopfli == 0 ||
      opt.filter_strategies != NULL || opt.keepchunks != NULL ||
      opt.block_split_strategy != 1) {
    fprintf(stderr, "CZopfliPNGSetDefaults produced unexpected values\n");
    return 1;
  }

  if (argc < 2) {
    return 0;
  }

  f = fopen(argv[1], "rb");
  if (!f) {
    perror(argv[1]);
    return 1;
  }
  if (fseek(f, 0, SEEK_END) != 0) {
    perror("fseek");
    fclose(f);
    return 1;
  }
  n = ftell(f);
  if (n <= 0) {
    fprintf(stderr, "empty input\n");
    fclose(f);
    return 1;
  }
  rewind(f);
  in = (unsigned char *)malloc((size_t)n);
  if (!in) {
    fclose(f);
    return 1;
  }
  if (fread(in, 1, (size_t)n, f) != (size_t)n) {
    fprintf(stderr, "short read\n");
    fclose(f);
    free(in);
    return 1;
  }
  fclose(f);

  rc = CZopfliPNGOptimize(in, (size_t)n, &opt, 0, &out, &outn);
  printf("CZopfliPNGOptimize rc=%d in=%ld out=%zu\n", rc, n, outn);
  free(in);
  free(out);
  if (rc != 0) {
    return 1;
  }
  if (outn >= (size_t)n) {
    fprintf(stderr, "did not shrink\n");
    return 1;
  }
  return 0;
}
