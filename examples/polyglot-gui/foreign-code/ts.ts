// TypeScript module — scriptc library mode.
//
// equilibrium compiles this file for the dashboard: it derives a scriptc
// library profile and a matching C header from the exports below (plus the
// marshalling classes in ../equilibrium.toml), runs
// `scriptc build --lib --profile …`, and links the archive it produces.
// The file stem becomes the symbol prefix, so `digit_sum` is `ts_digit_sum`.

export function digit_sum(value: number): number {
  let n = Math.trunc(value);
  if (n < 0) {
    n = -n;
  }
  let sum = 0;
  while (n > 0) {
    sum += n % 10;
    n = Math.floor(n / 10);
  }
  return sum;
}

export function sum(data: Uint8Array): number {
  let total = 0;
  for (let i = 0; i < data.length; i++) {
    total += data[i];
  }
  return total;
}

export function upper(text: string): string {
  return text.toUpperCase();
}

// `mix` takes its marshalling classes from ../equilibrium.toml (u32 params).
export function mix(tag: number, idx: number): number {
  return tag * 1000 + idx;
}
