package tools.plugin;

import java.io.*;
import java.net.*;

/** Redirects only plugin bytecode references; the reference ImageJ JAR is unchanged. */
final class DialogClassLoader extends URLClassLoader {
    DialogClassLoader(URL[] urls, ClassLoader parent) { super(urls, parent); }

    @Override
    protected Class<?> findClass(String name) throws ClassNotFoundException {
        URL resource = findResource(name.replace('.', '/') + ".class");
        if (resource == null) throw new ClassNotFoundException(name);
        try (InputStream stream = resource.openStream()) {
            byte[] bytes = redirectDialog(stream.readAllBytes());
            return defineClass(name, bytes, 0, bytes.length);
        } catch (IOException e) {
            throw new ClassNotFoundException(name, e);
        }
    }

    private static byte[] redirectDialog(byte[] bytes) throws IOException {
        DataInputStream in = new DataInputStream(new ByteArrayInputStream(bytes));
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        DataOutputStream out = new DataOutputStream(buffer);
        out.writeInt(in.readInt());
        out.writeShort(in.readUnsignedShort());
        out.writeShort(in.readUnsignedShort());
        int count = in.readUnsignedShort();
        out.writeShort(count);
        for (int i = 1; i < count; i++) {
            int tag = in.readUnsignedByte();
            out.writeByte(tag);
            switch (tag) {
                case 1:
                    String value = in.readUTF();
                    if (value.equals("ij/gui/GenericDialog")) value = "tools/plugin/ParameterDialog";
                    else value = value.replace("Lij/gui/GenericDialog;", "Ltools/plugin/ParameterDialog;");
                    out.writeUTF(value);
                    break;
                case 3: case 4: case 9: case 10: case 11: case 12: case 17: case 18:
                    out.writeInt(in.readInt()); break;
                case 5: case 6:
                    out.writeLong(in.readLong()); i++; break;
                case 7: case 8: case 16: case 19: case 20:
                    out.writeShort(in.readUnsignedShort()); break;
                case 15:
                    out.writeByte(in.readUnsignedByte()); out.writeShort(in.readUnsignedShort()); break;
                default: throw new IOException("Unsupported class constant tag: " + tag);
            }
        }
        out.write(in.readAllBytes());
        return buffer.toByteArray();
    }
}
