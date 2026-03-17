from main import add, classify

def test_add_positive():
    assert add(1, 2) == 3

def test_classify_positive():
    assert classify(5) == "positive"
