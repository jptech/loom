#!/usr/bin/env python3
"""Generate ALU test vectors as a SystemVerilog include file.

Usage:
    python3 gen_test_vectors.py [--width 8] [--count 64] [--output vectors.svh]

Produces alu_test_vectors.svh with stimulus arrays and expected results
for all ALU operations.
"""

import argparse
import os
import random


def alu_op(a, b, op, width):
    """Compute ALU result matching the RTL model."""
    mask = (1 << width) - 1
    if op == 0:    # ADD
        return (a + b) & mask
    elif op == 1:  # SUB
        return (a - b) & mask
    elif op == 2:  # AND
        return a & b
    elif op == 3:  # OR
        return a | b
    elif op == 4:  # XOR
        return a ^ b
    elif op == 5:  # NOT
        return (~a) & mask
    else:
        return 0


def main():
    parser = argparse.ArgumentParser(description="Generate ALU test vectors")
    parser.add_argument("--width", type=int, default=8, help="ALU data width")
    parser.add_argument("--count", type=int, default=64, help="Number of test vectors per operation")
    parser.add_argument("--output", default="generated/alu_test_vectors.svh", help="Output .svh file")
    parser.add_argument("--seed", type=int, default=42, help="Random seed for reproducibility")
    args = parser.parse_args()

    random.seed(args.seed)
    mask = (1 << args.width) - 1
    ops = [("ADD", 0), ("SUB", 1), ("AND", 2), ("OR", 3), ("XOR", 4), ("NOT", 5)]

    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)

    with open(args.output, "w") as f:
        f.write(f"// Auto-generated ALU test vectors — DO NOT EDIT\n")
        f.write(f"// Width={args.width}, Count={args.count}, Seed={args.seed}\n\n")

        f.write(f"localparam int TV_WIDTH = {args.width};\n")
        f.write(f"localparam int TV_COUNT = {args.count};\n\n")

        for op_name, op_code in ops:
            f.write(f"// {op_name} test vectors\n")
            f.write(f"logic [{args.width-1}:0] tv_{op_name.lower()}_a [{args.count}];\n")
            f.write(f"logic [{args.width-1}:0] tv_{op_name.lower()}_b [{args.count}];\n")
            f.write(f"logic [{args.width-1}:0] tv_{op_name.lower()}_exp [{args.count}];\n")
            f.write(f"initial begin\n")
            for i in range(args.count):
                a = random.randint(0, mask)
                b = random.randint(0, mask)
                exp = alu_op(a, b, op_code, args.width)
                f.write(f"  tv_{op_name.lower()}_a[{i}] = {args.width}'h{a:0{(args.width+3)//4}X}; ")
                f.write(f"tv_{op_name.lower()}_b[{i}] = {args.width}'h{b:0{(args.width+3)//4}X}; ")
                f.write(f"tv_{op_name.lower()}_exp[{i}] = {args.width}'h{exp:0{(args.width+3)//4}X};\n")
            f.write(f"end\n\n")

    print(f"  wrote {args.output} ({len(ops)} ops x {args.count} vectors)")


if __name__ == "__main__":
    main()
