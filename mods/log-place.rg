fn placed(id: String, x: Int, y: Int, z: Int) {
    print("rosegold: placed " + id + " at " + x + ", " + y + ", " + z);
}

fn main(): Int {
    hooks.after_place("log-place", placed);
    return 0;
}
