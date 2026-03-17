def add(a, b):
    if a < 0:
        return -1
    return a + b

def classify(n):
    if n > 0:
        return "positive"
    elif n == 0:
        return "zero"
    return "negative"
