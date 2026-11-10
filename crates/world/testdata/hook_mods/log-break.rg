fn broke(id: String, x: Int, y: Int, z: Int, left: String) {
    print("rosegold: broke " + id + " at " + x + ", " + y + ", " + z + " left " + left);
}

fn main(): Int {
    hooks.after_break("log-break", broke);
    return 0;
}
