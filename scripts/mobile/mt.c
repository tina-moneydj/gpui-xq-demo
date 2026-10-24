// mt：直接寫 /dev/input/eventN 的多點觸控手勢產生器（MT protocol B），有真實時間間隔。
// 用法（root）：mt DEV REPEAT GAPMS CMD ...
//   swipe x1 y1 x2 y2 ms        單指滑動（結尾直接放開 → 會甩動 fling）
//   drag  x1 y1 x2 y2 ms        單指拖曳，結尾停 150ms 再放開（不甩動）
//   hold  x y holdms x2 y2 ms   長按 holdms 後拖到 x2,y2（十字線）
//   pinch cx cy d0 d1 ms        兩指水平，間距 d0→d1（d1>d0 放大）
// 座標是螢幕像素（1080x2400），內部換算成 0..32767。
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static int fd; static int W=1080,H=2400; static int tid=100;
static void ev(int t,int c,int v){struct input_event e;memset(&e,0,sizeof e);e.type=t;e.code=c;e.value=v;write(fd,&e,sizeof e);}
static void syn(void){ev(EV_SYN,SYN_REPORT,0);}
static int sx(double x){return (int)(x*32767.0/(W-1));} static int sy(double y){return (int)(y*32767.0/(H-1));}
static void msleep(double ms){struct timespec t={(time_t)(ms/1000),(long)((ms-(long)(ms/1000)*1000)*1e6)};nanosleep(&t,0);}
static void down(int s,double x,double y){ev(EV_ABS,ABS_MT_SLOT,s);ev(EV_ABS,ABS_MT_TRACKING_ID,tid++);ev(EV_ABS,ABS_MT_POSITION_X,sx(x));ev(EV_ABS,ABS_MT_POSITION_Y,sy(y));ev(EV_ABS,ABS_MT_PRESSURE,200);}
static void mv(int s,double x,double y){ev(EV_ABS,ABS_MT_SLOT,s);ev(EV_ABS,ABS_MT_POSITION_X,sx(x));ev(EV_ABS,ABS_MT_POSITION_Y,sy(y));}
static void up(int s){ev(EV_ABS,ABS_MT_SLOT,s);ev(EV_ABS,ABS_MT_TRACKING_ID,-1);}
static const double STEP=8.0;
static void line(double x1,double y1,double x2,double y2,double ms){int n=ms/STEP;if(n<1)n=1;for(int i=1;i<=n;i++){double f=(double)i/n;mv(0,x1+(x2-x1)*f,y1+(y2-y1)*f);syn();msleep(STEP);}}
int main(int c,char**a){
  if(c<5){fprintf(stderr,"usage\n");return 2;}
  fd=open(a[1],O_RDWR); if(fd<0){perror("open");return 1;}
  int rep=atoi(a[2]); double gap=atof(a[3]); char*cmd=a[4]; double v[8]={0}; for(int i=0;i<8&&5+i<c;i++)v[i]=atof(a[5+i]);
  for(int r=0;r<rep;r++){
    if(!strcmp(cmd,"swipe")||!strcmp(cmd,"drag")){
      down(0,v[0],v[1]);ev(EV_KEY,BTN_TOUCH,1);syn();msleep(STEP);
      line(v[0],v[1],v[2],v[3],v[4]);
      if(!strcmp(cmd,"drag")){msleep(150);}
      up(0);ev(EV_KEY,BTN_TOUCH,0);syn();
    }else if(!strcmp(cmd,"hold")){
      down(0,v[0],v[1]);ev(EV_KEY,BTN_TOUCH,1);syn();msleep(v[2]);
      line(v[0],v[1],v[3],v[4],v[5]);msleep(100);
      up(0);ev(EV_KEY,BTN_TOUCH,0);syn();
    }else if(!strcmp(cmd,"pinch")){
      double cx=v[0],cy=v[1],d0=v[2],d1=v[3],ms=v[4];int n=ms/STEP;if(n<1)n=1;
      down(0,cx-d0/2,cy);down(1,cx+d0/2,cy);ev(EV_KEY,BTN_TOUCH,1);syn();msleep(STEP*2);
      for(int i=1;i<=n;i++){double d=d0+(d1-d0)*i/n;mv(0,cx-d/2,cy);mv(1,cx+d/2,cy);syn();msleep(STEP);}
      msleep(50);up(0);up(1);ev(EV_KEY,BTN_TOUCH,0);syn();
    }else{fprintf(stderr,"bad cmd\n");return 2;}
    msleep(gap);
  }
  return 0;
}
