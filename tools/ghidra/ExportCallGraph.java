// Export the program's static call graph (caller entry, call site, callee
// entry, names) and every function's body size. Outputs contain
// original-game evidence and must remain inside ignored private/.
// @category GDL.Research
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportCallGraph extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) throw new IllegalArgumentException("output directory required");
        File out = new File(args[0]);
        if (!out.isDirectory() && !out.mkdirs()) throw new IOException("cannot create output");
        FunctionManager fm = currentProgram.getFunctionManager();
        int edges = 0, functions = 0;
        try (PrintWriter calls = new PrintWriter(new File(out, "calls.tsv"), StandardCharsets.UTF_8);
             PrintWriter sizes = new PrintWriter(new File(out, "sizes.tsv"), StandardCharsets.UTF_8)) {
            calls.println("caller\tcaller_name\tsite\tcallee\tcallee_name");
            sizes.println("entry\tname\tbytes");
            FunctionIterator iter = fm.getFunctions(true);
            while (iter.hasNext()) {
                monitor.checkCancelled();
                Function f = iter.next(); functions++;
                sizes.println(f.getEntryPoint()+"\t"+f.getName()+"\t"+f.getBody().getNumAddresses());
                InstructionIterator insns = currentProgram.getListing().getInstructions(f.getBody(), true);
                while (insns.hasNext()) {
                    Instruction ins = insns.next();
                    for (Reference r : ins.getReferencesFrom()) {
                        if (!r.getReferenceType().isCall()) continue;
                        Function g = fm.getFunctionAt(r.getToAddress());
                        String name = g == null ? "?" : g.getName();
                        calls.println(f.getEntryPoint()+"\t"+f.getName()+"\t"+ins.getAddress()+"\t"+r.getToAddress()+"\t"+name);
                        edges++;
                    }
                }
            }
        }
        println("functions "+functions+" call edges "+edges);
    }
}
