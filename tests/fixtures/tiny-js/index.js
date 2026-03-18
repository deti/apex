function add(a, b) {
  if (a < 0) return -1;
  return a + b;
}

function classify(n) {
  if (n > 0) return "positive";
  if (n === 0) return "zero";
  return "negative";
}

module.exports = { add, classify };
