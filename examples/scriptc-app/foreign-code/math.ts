// scriptc (TypeScript) module for the equilibrium demo.
//
// Every `export function` becomes a C symbol prefixed with the module stem
// (`math_add`), and `equilibrium.toml` refines the marshalling classes.

export function add(a: number, b: number): number {
  return a + b;
}

export function greet(who: string, loud: boolean): string {
  return loud ? who.toUpperCase() : who;
}

export function sum(data: Uint8Array): number {
  let total = 0;
  for (let i = 0; i < data.length; i++) {
    total += data[i];
  }
  return total;
}

// `mix` and `truncate` take their marshalling classes from equilibrium.toml,
// so the host passes C-native integers instead of doubles.
export function mix(tag: number, idx: number): number {
  return tag * 1000 + idx;
}

// scriptc proves every integer return whole and in range, which the ordered
// comparisons establish here.
export function truncate(value: number): number {
  if (value > -9007199254740991 && value < 9007199254740991) {
    return Math.trunc(value);
  }
  return 0;
}
