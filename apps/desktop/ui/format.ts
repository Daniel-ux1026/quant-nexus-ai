const locale = "es-PE";
const integer = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 });
const separator = new Intl.NumberFormat(locale).formatToParts(1.1)
  .find((part) => part.type === "decimal")?.value ?? ".";

export function formatDecimal(value: string, minimumDecimals = 0, maximumDecimals = 28) {
  const [whole, fraction = ""] = value.split(".");
  const decimals = fraction.slice(0, maximumDecimals).replace(/0+$/, "").padEnd(minimumDecimals, "0");
  const prefix = integer.format(whole === "-0" ? -0 : BigInt(whole));
  return decimals ? `${prefix}${separator}${decimals}` : prefix;
}
