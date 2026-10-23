// Export every reference to the given addresses (function, site, type,
// instruction). Outputs contain original-game evidence and must remain
// inside ignored private/.
// @category GDL.Research
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class ExportRefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) throw new IllegalArgumentException("output file and addresses required");
        try (PrintWriter out = new PrintWriter(new File(args[0]), StandardCharsets.UTF_8)) {
            out.println("target\tfunction\tsite\ttype\tinstruction");
            for (int i = 1; i < args.length; i++) {
                Address a = toAddr(Long.decode(args[i]));
                for (Reference r : getReferencesTo(a)) {
                    Address site = r.getFromAddress();
                    Function f = getFunctionContaining(site);
                    Instruction ins = getInstructionAt(site);
                    out.println(a+"\t"+(f == null ? "?" : f.getName())+"\t"+site+"\t"+r.getReferenceType()+"\t"+(ins == null ? "" : ins.toString()));
                }
            }
        }
    }
}
