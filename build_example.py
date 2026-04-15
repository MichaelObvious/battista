from datetime import datetime, timedelta
from random import choice, random, randint
from subprocess import run

def dump(path: str, content: str):
    with open(path, 'w') as f:
        f.write(content)

CATEGORIES = ["Grocery", "Shopping", "Utilities", "Sport", "Transportation", "Restaurants", "Travel"]
NOTES = {
    "Grocery": ["Shop", "Market"],
    "Shopping": ["A clothing store"],
    "Utilities": ["Electricity", "Water"],
    "Sport": ["Equipement", "Facilities"],
    "Transportation": ["Public transport abo", "Train", "Tram", "Metro"],
    "Restaurants": ["Fancy", "Mexican", "Chinese", "Italian", "Sushi", "Pizza", "Coffee"],
    "Travel": ["Train", "Airplane", "Cruise"]
}
assert(len(CATEGORIES)== len(NOTES))
PAYMENT_METHODS = ["Debit Card", "Cash", "Credit Card"]

if __name__ == "__main__":
    today = datetime.today()
    start_date = today - timedelta(365.25 * 5)
    
    current_date = start_date
    current_budget = 1000
    budget_days_interval = 30
    lines = []
    lines.append(f'<budget amount="{current_budget:.2f}" duration="{budget_days_interval}" date="{start_date.strftime("%d/%m/%Y")}"/>')
    while current_date <= today:
        rn = random()
        if rn < 0.005:
            current_budget *= random() + 0.67
        elif rn < 0.07:
            lines.append(f'<budget category="{choice(CATEGORIES)}" amount="{current_budget*random()*0.33:.2f}" duration="{budget_days_interval}" date="{current_date.strftime("%d/%m/%Y")}"/>')
        max_expenses = 5 if random() < 0.67 else 30
        fraction = 0.6 if random() < 0.5 else 0.1
        for _ in range(randint(0, max_expenses)):
            category = choice(CATEGORIES)
            note = choice(NOTES[category])
            pm = choice(PAYMENT_METHODS)
            lines.append(f'<transaction amount="{random() * current_budget * fraction/budget_days_interval:.2f}" category="{category}" date="{current_date.strftime("%d/%m/%Y")}" payment-method="{pm}" note="{note}">')
        
        current_date += timedelta(1)
    lines.reverse()
    dump("./example/example.xml", '\n'.join(lines))
    
    run(["mkdir", "example"])
    run(["cargo", "run", "--release", "./example/example.xml"])
    run(["typst", "compile", "./example/example.typ"])
