/*
 * Differential oracle driver for libzopfli.
 * Uses only the public header src/zopfli/zopfli.h.
 */
#include "../src/zopfli/zopfli.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_INPUT 4096

static int want_section(int nsec, char **secs, const char *name) {
  int i;
  if (nsec == 0) return 1;
  for (i = 0; i < nsec; i++) {
    if (strcmp(secs[i], name) == 0) return 1;
  }
  return 0;
}

static void print_hex(const unsigned char *data, size_t n) {
  size_t i;
  for (i = 0; i < n; i++) {
    printf("%02x", data[i]);
  }
  printf("\n");
}

static int emit_format(const char *name, ZopfliFormat fmt,
                       const ZopfliOptions *opt,
                       const unsigned char *in, size_t insize) {
  unsigned char *out = 0;
  size_t outsize = 0;
  ZopfliCompress(opt, fmt, in, insize, &out, &outsize);
  if (outsize == 0 || out == 0) {
    fprintf(stderr, "compress failed for %s\n", name);
    free(out);
    return 1;
  }
  printf("=== %s ===\n", name);
  printf("len %zu\n", outsize);
  printf("hex ");
  print_hex(out, outsize);
  free(out);
  return 0;
}

static int parse_sections(char *arg, char ***out_secs, int *out_n) {
  char *p;
  int n = 1;
  int i;
  for (p = arg; *p; p++) {
    if (*p == ',') n++;
  }
  *out_secs = (char **)malloc((size_t)n * sizeof(char *));
  if (!*out_secs) return -1;
  (*out_secs)[0] = arg;
  i = 1;
  for (p = arg; *p; p++) {
    if (*p == ',') {
      *p = '\0';
      (*out_secs)[i++] = p + 1;
    }
  }
  *out_n = n;
  return 0;
}

int main(int argc, char **argv) {
  const char *path = 0;
  char **secs = 0;
  int nsec = 0;
  int i;
  FILE *f;
  unsigned char *in = 0;
  size_t insize = 0;
  long flen;
  ZopfliOptions opt;
  int rc = 0;

  for (i = 1; i < argc; i++) {
    if (strcmp(argv[i], "--sections") == 0) {
      if (i + 1 >= argc) {
        fprintf(stderr, "missing --sections value\n");
        return 2;
      }
      if (parse_sections(argv[++i], &secs, &nsec) != 0) return 2;
    } else if (!path) {
      path = argv[i];
    } else {
      fprintf(stderr, "unexpected arg %s\n", argv[i]);
      free(secs);
      return 2;
    }
  }

  if (!path) {
    fprintf(stderr, "usage: oracle [--sections a,b] <fixture>\n");
    free(secs);
    return 2;
  }

  f = fopen(path, "rb");
  if (!f) {
    fprintf(stderr, "cannot open %s\n", path);
    free(secs);
    return 2;
  }
  if (fseek(f, 0, SEEK_END) != 0) {
    fclose(f);
    free(secs);
    return 2;
  }
  flen = ftell(f);
  if (flen < 0) {
    fclose(f);
    free(secs);
    return 2;
  }
  if (fseek(f, 0, SEEK_SET) != 0) {
    fclose(f);
    free(secs);
    return 2;
  }
  insize = (size_t)flen;
  if (insize > 0) {
    in = (unsigned char *)malloc(insize);
    if (!in || fread(in, 1, insize, f) != insize) {
      fprintf(stderr, "read failed\n");
      free(in);
      fclose(f);
      free(secs);
      return 2;
    }
  }
  fclose(f);

  if (insize > MAX_INPUT) {
    printf("SKIP oversize %zu\n", insize);
    free(in);
    free(secs);
    return 0;
  }

  ZopfliInitOptions(&opt);
  opt.numiterations = 1;
  opt.verbose = 0;
  opt.verbose_more = 0;

  if (want_section(nsec, secs, "gzip")) {
    if (emit_format("gzip", ZOPFLI_FORMAT_GZIP, &opt, in, insize) != 0) rc = 1;
  }
  if (rc == 0 && want_section(nsec, secs, "zlib")) {
    if (emit_format("zlib", ZOPFLI_FORMAT_ZLIB, &opt, in, insize) != 0) rc = 1;
  }
  if (rc == 0 && want_section(nsec, secs, "deflate")) {
    if (emit_format("deflate", ZOPFLI_FORMAT_DEFLATE, &opt, in, insize) != 0)
      rc = 1;
  }

  free(in);
  free(secs);
  return rc;
}
