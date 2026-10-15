// Export a bounded set of original-symbol functions and recovered type layouts.
// Outputs contain original-game evidence and must remain inside ignored private/.
// @category GDL.Research
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.*;
import ghidra.program.model.data.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public class ExportResearch extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length == 0) throw new IllegalArgumentException("output directory required");
        File out = new File(args[0]);
        if (!out.isDirectory() && !out.mkdirs()) throw new IOException("cannot create output");
        Set<String> wanted = new HashSet<>(Arrays.asList(args).subList(1, args.length));
        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);
        int functions = 0, exported = 0, structures = 0;
        try (PrintWriter symbols = new PrintWriter(new File(out, "functions.tsv"), StandardCharsets.UTF_8);
             PrintWriter types = new PrintWriter(new File(out, "structures.tsv"), StandardCharsets.UTF_8);
             PrintWriter report = new PrintWriter(new File(out, "summary.txt"), StandardCharsets.UTF_8)) {
            symbols.println("guest_address\tname\tprototype\tsource");
            FunctionIterator iter = currentProgram.getFunctionManager().getFunctions(true);
            while (iter.hasNext()) {
                monitor.checkCancelled();
                Function f = iter.next(); functions++;
                symbols.println(f.getEntryPoint()+"\t"+f.getName()+"\t"+f.getPrototypeString(false,false)+"\t"+f.getSymbol().getSource());
                // "@00123456" selects an exact entry point for label-only helpers
                // such as gcc2_compiled.; it never guesses a name for that entry.
                if (!wanted.contains(f.getName()) && !wanted.contains("@"+f.getEntryPoint())) continue;
                String safe = f.getName().replaceAll("[^A-Za-z0-9_]", "_");
                try (PrintWriter file = new PrintWriter(new File(out, f.getEntryPoint()+"-"+safe+".txt"), StandardCharsets.UTF_8)) {
                    file.println("Language: "+currentProgram.getLanguageID());
                    file.println("Compiler: "+currentProgram.getCompilerSpec().getCompilerSpecID());
                    file.println("Original symbol: "+f.getName()+" @ EE:"+f.getEntryPoint());
                    file.println("Prototype: "+f.getPrototypeString(false,false));
                    InstructionIterator insns = currentProgram.getListing().getInstructions(f.getBody(), true);
                    while(insns.hasNext()) {
                        Instruction ins = insns.next();
                        file.println(ins.getAddress()+"\t"+ins.toString());
                    }
                    file.println("\nDECOMPILER INTERPRETATION (verify against instructions)");
                    DecompileResults result = decompiler.decompileFunction(f, 30, monitor);
                    if (result.decompileCompleted() && result.getDecompiledFunction()!=null) file.println(result.getDecompiledFunction().getC());
                    else file.println("UNAVAILABLE: "+result.getErrorMessage());
                }
                exported++;
            }
            types.println("type\tsize\tfield_offset\tfield_size\tfield_name\tfield_type");
            Iterator<DataType> data = currentProgram.getDataTypeManager().getAllDataTypes();
            while(data.hasNext()) {
                monitor.checkCancelled();
                DataType dt = data.next();
                if (!(dt instanceof Structure)) continue;
                structures++;
                for(DataTypeComponent c : ((Structure)dt).getDefinedComponents()) {
                    types.println(dt.getPathName()+"\t"+dt.getLength()+"\t"+c.getOffset()+"\t"+c.getLength()+"\t"+c.getFieldName()+"\t"+c.getDataType().getPathName());
                }
            }
            report.println("Executable: "+currentProgram.getExecutablePath());
            report.println("Language: "+currentProgram.getLanguageID());
            report.println("Compiler: "+currentProgram.getCompilerSpec().getCompilerSpecID());
            report.println("Functions: "+functions);
            report.println("Selected exports: "+exported);
            report.println("Recovered structures: "+structures);
            report.println("Missing requested names are unresolved, not guessed.");
        } finally { decompiler.dispose(); }
        println("Exported "+exported+" selected functions; "+structures+" structure layouts");
    }
}
