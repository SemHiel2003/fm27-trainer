import sys, struct, subprocess, os
def zstd_d(b):
    return subprocess.run(["zstd","-d","-c","-q"],input=b,capture_output=True,check=True).stdout
def aes_ctr(key, iv, ct):
    return subprocess.run(["openssl","enc","-aes-128-ctr","-d","-nopad","-K",key.hex(),"-iv",iv.hex()],input=ct,capture_output=True,check=True).stdout
def rd_str(m,p):
    n=struct.unpack_from("<I",m,p)[0]; p+=4; return m[p:p+n].decode("utf-8","replace"), p+n
def rd_dir(m,p,path,out):
    nf=struct.unpack_from("<I",m,p)[0]; p+=4
    for _ in range(nf):
        name,p=rd_str(m,p); ext,p=rd_str(m,p)
        off,size,raw,t1,t2=struct.unpack_from("<QQQqq",m,p); p+=40
        out.append((path+name+ext,off,size,raw))
    nd=struct.unpack_from("<I",m,p)[0]; p+=4
    for _ in range(nd):
        name,p=rd_str(m,p); p=rd_dir(m,p,path+name+"/",out)
    return p
def unpack(fn, outdir):
    d=open(fn,"rb").read()
    enc = d[2:6]==b"afe."
    assert enc or d[2:6]==b"fmf.", d[:16]
    A=struct.unpack_from("<Q",d,9)[0]
    assert d[A+11:A+15]==b"fmf.", d[A+9:A+18]
    m=zstd_d(d[A+18:])
    root,p=rd_str(m,0); files=[]; p=rd_dir(m,p,"",files)
    print(f"{fn}: enc={enc} root={root!r} files={len(files)} manifest_used={p}/{len(m)}")
    os.makedirs(outdir,exist_ok=True)
    for name,off,size,raw in files:
        rec=d[26+off:26+off+size]
        if enc:
            kl,il=struct.unpack_from("<II",rec,0)
            key=rec[8:8+kl]; iv=rec[8+kl:8+kl+il]; ct=rec[8+kl+il:]
            z=aes_ctr(key,iv,ct)
        else:
            z=rec
        try: plain=zstd_d(z)
        except Exception as e: print("  zstd fail",name,z[:8].hex()); continue
        print(f"  {name:50s} off={off:8d} size={size:8d} raw={raw:8d} got={len(plain)} magic={plain[:10]!r}")
        p2=os.path.join(outdir,name.replace("/","__")); open(p2,"wb").write(plain)
if __name__=="__main__":
    unpack(sys.argv[1], sys.argv[2])
