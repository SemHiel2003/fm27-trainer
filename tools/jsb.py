import sys, struct, json
class R:
    def __init__(s,b): s.b=b; s.p=0
    def u8(s): v=s.b[s.p]; s.p+=1; return v
    def u32(s): v=struct.unpack_from("<I",s.b,s.p)[0]; s.p+=4; return v
    def i32(s): v=struct.unpack_from("<i",s.b,s.p)[0]; s.p+=4; return v
    def raw(s,n): v=s.b[s.p:s.p+n]; s.p+=n; return v
unknown=set()
def val(r):
    t=r.u8(); ty=t&0xf; n=t>>4
    if ty==0xa:  # object
        cnt = r.u32() if n==0 else n-1
        o={}
        for _ in range(cnt):
            kl=r.u8(); k=r.raw(kl).decode()
            o[k]=val(r)
        return o
    if ty==0x9:  # array
        cnt = r.u32() if n==0 else n-1
        return [val(r) for _ in range(cnt)]
    if ty==0x8:
        ln = r.u32() if n==0 else n-1
        return r.raw(ln).decode("utf-8","replace")
    if ty==0x1: return bool(n)
    if ty==0x2: return r.i32() if n==0 else ("i%d"%(n-1))
    unknown.add(hex(t)); return "?%02x@%d"%(t,r.p-1)
d=open(sys.argv[1],"rb").read()
r=R(d); v=val(r)
print(json.dumps(v,indent=1,ensure_ascii=False)[: int(sys.argv[2]) if len(sys.argv)>2 else 10**9])
print("end",r.p,len(d),"unknown",unknown, file=sys.stderr)
